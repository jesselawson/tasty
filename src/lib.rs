// Copyright (c) 2025 Jesse Lawson <jesse@lawsonry.com>
// GNU General Public License v3.0+ (see LICENSE or https://www.gnu.org/licenses/gpl-3.0.txt)

use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use toml::Table;

#[derive(Parser, Debug)]
#[command(author, version, about = format!("{}, the API server testing tool", "Tasty".bold()))]
pub struct Args {
    /// Base URL for the API (defaults to http://127.0.0.1:3030)
    #[arg(short = 'b', long = "base-url", value_name = "BASE_URL")]
    pub base_url: Option<String>,

    /// Custom tests folder path
    #[arg(short = 't', long = "tests-folder", value_name = "FOLDER")]
    pub tests_folder: Option<PathBuf>,

    /// Global timeout in seconds
    #[arg(
        short = 'g',
        long = "global-timeout",
        value_name = "SECONDS",
        default_value = "30"
    )]
    pub timeout: u64,

    /// Prints extra information on test run, including responses for
    /// passing tests.
    #[arg(short = 'd', long = "debug")]
    pub debug: bool,

    /// Output results as JSON (Not implemented yet)
    #[arg(short = 'j', long = "json")]
    pub json: bool,

    /// Specific test files to run
    #[arg(value_name = "TESTS")]
    pub test_files: Vec<String>,
}

/// Configuration for test expectations
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExpectConfig {
    /// The HTTP status code expected for a passing test
    pub http_status: u16,

    /// Properties expected in the response object (literal matching)
    #[serde(default)]
    pub response: Option<Table>,

    /// Properties expected in the response object (regex matching)
    #[serde(default)]
    pub response_regex: Option<Table>,

    /// Properties expected in response headers (future use)
    #[serde(default)]
    pub headers: Option<Table>,
}

/// Represents a reference to a value from a previous test's response
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValueReference {
    /// The name of the test to reference
    pub from: String,
    /// The property path to extract (e.g., "response.access_token")
    pub property: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestCase {
    /// The name of the test, set from the toml table key value if not explicitly provided
    #[serde(default)]
    pub name: String,

    /// The HTTP method used in this test
    pub method: String,

    /// The route (e.g., `{base_url}{route}`) to
    /// send the request
    pub route: String,

    /// The data payload to send as part of the request.
    /// It will be in `application/json` format.
    pub payload: serde_json::Value,

    /// Test expectations configuration
    pub expect: ExpectConfig,

    /// `None` The test hasn't run yet
    /// `true` The test passed
    /// `false` The test failed
    #[serde(skip)]
    outcome: Option<bool>,

    /// The time it took for the test to complete its run
    #[serde(skip)]
    duration: Option<Duration>,

    /// Information related to why this test failed
    #[serde(skip)]
    feedback: Option<String>,
}

/// Implicit conversion implementation to provide JSON intermediate represenation 
/// for type casting as well as future export-to-json work
impl TryFrom<&toml::Value> for TestCase {
    type Error = anyhow::Error;
    
    fn try_from(value: &toml::Value) -> std::result::Result<Self, Self::Error> {
        if let toml::Value::Table(_) = value {
            let test_case: TestCase = serde_json::from_value(
                serde_json::to_value(value)?
            )?;
            return Ok(test_case);
        } 
        Err(anyhow::anyhow!("Expected TOML table"))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TestStats {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub total_duration: Duration,
}

#[derive(Debug, Serialize)]
pub struct TestResult {
    pub name: String,
    pub status: TestStatus,
    pub duration_ms: Option<u128>,
    pub feedback: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Serialize)]
pub struct TestSuiteResult {
    pub stats: TestStats,
    pub results: Vec<TestResult>,
    pub success: bool,
}

// Make TestStats serializable for JSON output
impl Serialize for TestStats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("TestStats", 5)?;
        state.serialize_field("total", &self.total)?;
        state.serialize_field("passed", &self.passed)?;
        state.serialize_field("failed", &self.failed)?;
        state.serialize_field("skipped", &self.skipped)?;
        state.serialize_field("total_duration_ms", &self.total_duration.as_millis())?;
        state.end()
    }
}

static METHODS: [&str; 5] = ["POST", "GET", "PUT", "PATCH", "DELETE"];

/// Extracts a nested value from a JSON Value using dot notation.
/// For example, "response.user.id" would traverse response -> user -> id.
fn get_nested_value<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        current = current.get(part)?;
    }

    Some(current)
}

/// Checks if a value is a reference object (has "from" and "property" fields)
fn is_value_reference(value: &serde_json::Value) -> bool {
    if let serde_json::Value::Object(obj) = value {
        obj.contains_key("from") && obj.contains_key("property") && obj.len() == 2
    } else {
        false
    }
}

/// Resolves all value references in a payload using stored responses from previous tests.
/// Returns an error if a referenced test doesn't exist or the property path is invalid.
fn resolve_payload_references(
    payload: &mut serde_json::Value,
    responses: &HashMap<String, serde_json::Value>,
) -> Result<()> {
    match payload {
        serde_json::Value::Object(obj) => {
            let keys: Vec<String> = obj.keys().cloned().collect();
            for key in keys {
                if let Some(value) = obj.get(&key) {
                    if is_value_reference(value) {
                        // Extract reference info
                        let from = value.get("from")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| anyhow::anyhow!("Invalid 'from' field in reference"))?;
                        let property = value.get("property")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| anyhow::anyhow!("Invalid 'property' field in reference"))?;

                        // Look up the referenced response
                        let referenced_response = responses.get(from)
                            .ok_or_else(|| anyhow::anyhow!(
                                "Referenced test '{}' not found. Make sure the test exists and ran successfully before this test.",
                                from
                            ))?;

                        // Extract the nested value
                        let resolved_value = get_nested_value(referenced_response, property)
                            .ok_or_else(|| anyhow::anyhow!(
                                "Property path '{}' not found in response from test '{}'",
                                property, from
                            ))?
                            .clone();

                        // Replace the reference with the resolved value
                        obj.insert(key, resolved_value);
                    } else {
                        // Recursively process nested objects/arrays
                        if let Some(nested) = obj.get_mut(&key) {
                            resolve_payload_references(nested, responses)?;
                        }
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                resolve_payload_references(item, responses)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Validates response against literal expected values.
/// Returns Ok(true) if all expectations match, Ok(false) with feedback if mismatch.
fn validate_response_literal(
    response: &serde_json::Value,
    expected: &Table,
) -> (bool, Option<String>) {
    let mut mismatches = Vec::new();

    for (key, expected_value) in expected.iter() {
        let expected_json: serde_json::Value = match serde_json::to_value(expected_value) {
            Ok(v) => v,
            Err(_) => {
                mismatches.push(format!("Failed to convert expected value for key '{}'", key));
                continue;
            }
        };

        // Support dot notation for nested access
        let actual_value = get_nested_value(response, key);

        match actual_value {
            Some(actual) if actual == &expected_json => {
                // Match - continue
            }
            Some(actual) => {
                mismatches.push(format!(
                    "Key '{}': expected '{}', got '{}'",
                    key,
                    expected_json,
                    actual
                ));
            }
            None => {
                mismatches.push(format!(
                    "Key '{}': expected '{}', but key not found in response",
                    key,
                    expected_json
                ));
            }
        }
    }

    if mismatches.is_empty() {
        (true, None)
    } else {
        (false, Some(mismatches.join("\n")))
    }
}

/// Validates response against regex patterns.
/// Returns Ok(true) if all patterns match, Ok(false) with feedback if mismatch.
fn validate_response_regex(
    response: &serde_json::Value,
    patterns: &Table,
) -> (bool, Option<String>) {
    let mut mismatches = Vec::new();

    for (key, pattern_value) in patterns.iter() {
        let pattern_str = match pattern_value.as_str() {
            Some(s) => s,
            None => {
                mismatches.push(format!("Regex pattern for key '{}' must be a string", key));
                continue;
            }
        };

        let regex = match Regex::new(pattern_str) {
            Ok(r) => r,
            Err(e) => {
                mismatches.push(format!("Invalid regex pattern for key '{}': {}", key, e));
                continue;
            }
        };

        // Support dot notation for nested access
        let actual_value = get_nested_value(response, key);

        match actual_value {
            Some(actual) => {
                // Convert the actual value to a string for regex matching
                let actual_str = match actual {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => "null".to_string(),
                    _ => actual.to_string(),
                };

                if !regex.is_match(&actual_str) {
                    mismatches.push(format!(
                        "Key '{}': value '{}' does not match pattern '{}'",
                        key, actual_str, pattern_str
                    ));
                }
            }
            None => {
                mismatches.push(format!(
                    "Key '{}': expected to match pattern '{}', but key not found in response",
                    key, pattern_str
                ));
            }
        }
    }

    if mismatches.is_empty() {
        (true, None)
    } else {
        (false, Some(mismatches.join("\n")))
    }
}

/// A helper function to print test results to the terminal window
pub fn print_summary(stats: &TestStats) {
    println!("\nTest Summary:");
    println!("  Total tests: {}", stats.total);
    println!("  Passed: {}", stats.passed.to_string().green());
    println!("  Failed: {}", stats.failed.to_string().red());
    println!("  Skipped: {}", stats.skipped.to_string().yellow());
    println!("  Total duration: {}ms", stats.total_duration.as_millis());
}

/// Scans the raw TOML content to extract table keys in the order they appear,
/// providing a vec ready to be iterated through by the test runner
fn extract_table_keys_order(content: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') // Should start with a '['
            && trimmed.ends_with(']') // Should end with a ']'
            && !trimmed[1..].contains('[')
        // Should not have s second '[' after pos 0
        {
            let key = trimmed[1..trimmed.len() - 1].to_string();
            keys.push(key);
        }
    }

    keys
}

pub fn get_test_files(args: &Args) -> Result<Vec<PathBuf>> {
    let base_tests_dir = args
        .tests_folder
        .clone()
        .unwrap_or_else(|| PathBuf::from("api_tests"));

    let mut tests_dir = base_tests_dir.clone();

    if !tests_dir.is_absolute() {
        if let Ok(current_dir) = std::env::current_dir() {
            tests_dir = current_dir.join(tests_dir);
        }
        if args.debug {
            println!(
                "{}",
                format!("Using tests directory: {:?}", tests_dir).dimmed()
            );
        }
    } else {
        return Err(anyhow::anyhow!("Failed to determine current directory"));
    }

    let mut test_files = Vec::new();

    if args.test_files.is_empty() {
        // Find all .toml files in the tests directory
        let read_dir = match fs::read_dir(&tests_dir) {
            Ok(d) => d,
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Unable to access the folder '{}'\n  (Got \"{}\")\nBe sure the folder exists and is readable. If you don't understand why you are getting this error, try the help command:\n    {}",
                    &tests_dir.display(),
                    e,
                    "tasty --help".blue().bold()
                ));
            }
        };

        for entry in read_dir {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                test_files.push(path);
            }
        }
    } else {
        // Look for specified test files
        for test_name in &args.test_files {
            let mut path = tests_dir.clone();
            path.push(format!("{}.toml", test_name));

            if path.exists() {
                test_files.push(path);
            } else {
                println!(
                    "{}",
                    format!("Warning: Test file '{}' not found", test_name).yellow()
                );
            }
        }
    }

    Ok(test_files)
}

pub async fn run_test_case(
    client: &Client,
    base_url: &str,
    test: &mut TestCase,
    args: &Args,
    responses: &HashMap<String, serde_json::Value>,
) -> Result<Option<serde_json::Value>> {
    let start_time = Instant::now();
    let url = format!("{}/{}", base_url.trim_end_matches('/'), test.route);

    // Validate HTTP method
    if !METHODS.iter().any(|m| test.method.contains(m)) {
        test.outcome = Some(false);
        test.feedback = Some(format!("Invalid HTTP method: {}", test.method));
        return Ok(None);
    }

    // Resolve any payload references from previous test responses
    let mut payload = test.payload.clone();
    if let Err(e) = resolve_payload_references(&mut payload, responses) {
        test.outcome = Some(false);
        test.feedback = Some(format!("    {}\n    {}", "Reference resolution failed:".bold(), e));
        return Ok(None);
    }

    // Execute the request
    let response = match client
        .request(test.method.parse()?, &url)
        .json(&payload)
        .timeout(Duration::from_secs_f64(args.timeout as f64))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            test.outcome = Some(false);
            test.feedback = Some(format!("Request failed: {}", e));
            return Err(e.into());
        }
    };

    let response_status = response.status().clone();

    let status_matches = response.status().as_u16() == test.expect.http_status;
    let actual_status = response.status().as_u16();

    // Parse and validate response
    let response_json: serde_json::Value = match response.json().await {
        Ok(json) => json,
        Err(e) => {
            test.outcome = Some(false);
            test.feedback = Some(format!(
                "    HTTP Response: {}\n      Error: {}",
                &response_status, e
            ));
            return Ok(None);
        }
    };

    if args.debug {
        println!(
            "{} {}\n{}\n{}",
            &test.name.blue().dimmed().bold(),
            "response".blue().dimmed(),
            &response_status,
            serde_json::to_string_pretty(&response_json)
                .unwrap()
                .dimmed()
        );
    }

    // Validate literal response expectations
    let (literal_matches, literal_feedback) = test
        .expect
        .response
        .as_ref()
        .map(|expected| validate_response_literal(&response_json, expected))
        .unwrap_or((true, None));

    // Validate regex response expectations
    let (regex_matches, regex_feedback) = test
        .expect
        .response_regex
        .as_ref()
        .map(|patterns| validate_response_regex(&response_json, patterns))
        .unwrap_or((true, None));

    // Record test results
    test.duration = Some(start_time.elapsed());
    test.outcome = Some(status_matches && literal_matches && regex_matches);

    if !test.outcome.unwrap() {
        let mut feedback = Vec::new();

        if !status_matches {
            feedback.push(format!(
                "    {}\n    Expected: {}\n    Returned: {}",
                "Status code mismatch:".bold(),
                test.expect.http_status.to_string().dimmed(),
                actual_status.to_string().dimmed()
            ));
        }

        if !literal_matches {
            feedback.push(format!(
                "    {}\n{}",
                "Literal expectation mismatch:".bold(),
                literal_feedback
                    .unwrap_or_default()
                    .lines()
                    .map(|l| format!("      {}", l))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .dimmed()
            ));
        }

        if !regex_matches {
            feedback.push(format!(
                "    {}\n{}",
                "Regex expectation mismatch:".bold(),
                regex_feedback
                    .unwrap_or_default()
                    .lines()
                    .map(|l| format!("      {}", l))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .dimmed()
            ));
        }

        // Also show the actual response for debugging
        feedback.push(format!(
            "    {}\n{}",
            "Actual response:".bold(),
            serde_json::to_string_pretty(&response_json)
                .unwrap()
                .lines()
                .map(|l| format!("      {}", l))
                .collect::<Vec<String>>()
                .join("\n")
                .dimmed()
        ));

        test.feedback = Some(feedback.join("\n"));
    }

    Ok(Some(response_json))
}

pub async fn run_tests(args: &Args) -> Result<TestSuiteResult> {
    let base_url = args
        .base_url
        .clone()
        .unwrap_or_else(|| "http://127.0.0.1:3030".to_string());

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("Failed to create HTTP client")?;

    if args.debug {
        println!(
            "Test folder: {:?}",
            &args
                .tests_folder
                .clone()
                .unwrap_or(std::path::PathBuf::from("api_tests"))
        );
    }

    let test_files = get_test_files(&args)?;
    let mut stats = TestStats::default();
    let mut results: Vec<TestResult> = Vec::new();
    let mut responses: HashMap<String, serde_json::Value> = HashMap::new();
    let suite_start = Instant::now();

    // Handle case where no test files were found
    if test_files.is_empty() {
        println!("{}", "No test files found to execute".yellow());

        return Ok(TestSuiteResult {
            stats,
            results,
            success: false,
        });
    }

    for path in test_files {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read test file: {:?}", path))?;

        let test_cases: Table = toml::from_str(&content)
            .with_context(|| format!("Failed to parse TOML from: {:?}", path))?;

        println!(
            "\n{}",
            format!("Running tests from {:?}\n", path.file_name().unwrap())
                .blue()
                .bold()
        );

        let total_tests = &test_cases.len();

        let ordered_keys = extract_table_keys_order(&content);

        for key in ordered_keys {
            if let Some(case) = test_cases.get(&key) {
                let mut test: TestCase = case.try_into()?;

                // Use the table key as the test name if none was provided
                if test.name.is_empty() {
                    test.name = key;
                }
                stats.total += 1;

                if !args.debug {
                    print!("  {} ... ", test.name);
                } else {
                    println!(
                        "{}",
                        format!(
                            "{} {}\n{}\n",
                            &test.name.blue().dimmed().bold(),
                            "definition".blue().dimmed(),
                            serde_json::to_string_pretty(&test).unwrap().dimmed()
                        )
                    );
                }

                let test_name = test.name.clone();
                let result = match run_test_case(&client, &base_url, &mut test, &args, &responses).await {
                    Err(e) => {
                        println!("{}", "ERROR".red());
                        println!("    {}", e);

                        stats.failed += 1;
                        TestResult {
                            name: test.name,
                            status: TestStatus::Error,
                            duration_ms: None,
                            feedback: Some(format!("    {}\n", e.to_string().dimmed().red())),
                        }
                    }
                    Ok(response_json) => {
                        // Store the response for potential reference by later tests
                        if let Some(json) = response_json {
                            responses.insert(test_name.clone(), json);
                        }

                        match test.outcome {
                            Some(true) => {
                                if args.debug {
                                    println!(
                                        "{}\n{}",
                                        format!(
                                            "{} {}",
                                            &test.name.blue().dimmed().bold(),
                                            "outcome".blue().dimmed(),
                                        ),
                                        format!(
                                            "    {} {}",
                                            "ok".green(),
                                            format!("({}ms)", test.duration.unwrap().as_millis())
                                                .dimmed()
                                        )
                                    );
                                } else {
                                    println!(
                                        "{} {}",
                                        "ok".green(),
                                        format!("({}ms)", test.duration.unwrap().as_millis()).dimmed()
                                    );
                                }

                                stats.passed += 1;
                                TestResult {
                                    name: test.name,
                                    status: TestStatus::Passed,
                                    duration_ms: Some(test.duration.unwrap().as_millis()),
                                    feedback: None,
                                }
                            }
                        Some(false) => {
                            if args.debug {
                                println!(
                                    "{}\n{}",
                                    format!(
                                        "{} {}",
                                        &test.name.blue().dimmed().bold(),
                                        "outcome".blue().dimmed(),
                                    ),
                                    "    failed".red()
                                );
                            } else {
                                println!("{}", "failed".red());
                            }
                            println!("{}", test.feedback.as_ref().unwrap());

                            stats.failed += 1;
                            TestResult {
                                name: test.name,
                                status: TestStatus::Failed,
                                duration_ms: None,
                                feedback: test.feedback,
                            }
                        }
                        None => {
                            println!("{}", "SKIPPED".yellow());

                            stats.skipped += 1;
                            TestResult {
                                name: test.name,
                                status: TestStatus::Skipped,
                                duration_ms: None,
                                feedback: None,
                            }
                        }
                    }
                    }
                };

                // If we can't run the first test, we likely can't run the rest of them.
                // Bail out and let the user know:
                match result.status {
                    TestStatus::Error => {
                        // How many tests are left?
                        stats.skipped += total_tests - stats.total;
                        println!(
                            "\n{}\n{}",
                            "Failed to run a test (can the API server be reached?)".red(),
                            format!("Skipping {} remaining tests.", stats.skipped).yellow()
                        );
                        break;
                    }
                    _ => {
                        results.push(result);
                    }
                }
            }
        }
    }

    stats.total_duration = suite_start.elapsed();

    print_summary(&stats);

    Ok(TestSuiteResult {
        stats,
        results,
        success: stats.failed == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TestCase, get_test_files};
    use anyhow::Result;
    use std::path::PathBuf;
    use toml::Table;

    #[test]
    fn test_deserialize_test_case() -> Result<()> {
        let test_file = PathBuf::from("examples/single_endpoint.toml");
        let content = std::fs::read_to_string(test_file)?;
        let test_cases: Table = toml::from_str(&content)?;

        let test_case: TestCase = test_cases["test_get_user"].clone().try_into()?;
        assert_eq!(test_case.method, "GET");
        assert_eq!(test_case.route, "api/users/1");
        Ok(())
    }

    #[test]
    fn test_file_discovery() -> Result<()> {
        let args = Args {
            base_url: None,
            test_files: vec![],
            tests_folder: Some(PathBuf::from("examples")),
            timeout: 30,
            json: false,
            debug: false,
        };

        let files = get_test_files(&args)?;

        assert!(files.iter().any(|f| f.ends_with("examples/error_cases.toml")));
        assert!(files.iter().any(|f| f.ends_with("examples/login_tests.toml")));
        assert!(files.iter().any(|f| f.ends_with("examples/single_endpoint.toml")));

        Ok(())
    }

    #[test]
    fn test_get_nested_value() {
        let json = serde_json::json!({
            "user": {
                "profile": {
                    "name": "John",
                    "age": 30
                },
                "id": 123
            },
            "token": "abc123"
        });

        // Test simple key access
        assert_eq!(get_nested_value(&json, "token"), Some(&serde_json::json!("abc123")));

        // Test nested key access
        assert_eq!(get_nested_value(&json, "user.id"), Some(&serde_json::json!(123)));
        assert_eq!(get_nested_value(&json, "user.profile.name"), Some(&serde_json::json!("John")));
        assert_eq!(get_nested_value(&json, "user.profile.age"), Some(&serde_json::json!(30)));

        // Test non-existent key
        assert_eq!(get_nested_value(&json, "nonexistent"), None);
        assert_eq!(get_nested_value(&json, "user.nonexistent"), None);
    }

    #[test]
    fn test_validate_response_literal() {
        let response = serde_json::json!({
            "status": "ok",
            "code": 200,
            "data": {
                "id": 1,
                "name": "Test"
            }
        });

        // Test matching values
        let mut expected = Table::new();
        expected.insert("status".to_string(), toml::Value::String("ok".to_string()));
        let (matches, feedback) = validate_response_literal(&response, &expected);
        assert!(matches, "Should match: {:?}", feedback);

        // Test non-matching values
        let mut expected = Table::new();
        expected.insert("status".to_string(), toml::Value::String("error".to_string()));
        let (matches, feedback) = validate_response_literal(&response, &expected);
        assert!(!matches, "Should not match");
        assert!(feedback.is_some());

        // Test nested access with dot notation
        let mut expected = Table::new();
        expected.insert("data.id".to_string(), toml::Value::Integer(1));
        let (matches, feedback) = validate_response_literal(&response, &expected);
        assert!(matches, "Should match nested: {:?}", feedback);
    }

    #[test]
    fn test_validate_response_regex() {
        let response = serde_json::json!({
            "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            "user_id": 12345,
            "email": "test@example.com"
        });

        // Test matching regex patterns
        let mut patterns = Table::new();
        patterns.insert("token".to_string(), toml::Value::String("[a-zA-Z0-9]+".to_string()));
        let (matches, feedback) = validate_response_regex(&response, &patterns);
        assert!(matches, "Should match regex: {:?}", feedback);

        // Test email regex
        let mut patterns = Table::new();
        patterns.insert("email".to_string(), toml::Value::String(r".*@.*\..*".to_string()));
        let (matches, feedback) = validate_response_regex(&response, &patterns);
        assert!(matches, "Should match email regex: {:?}", feedback);

        // Test numeric regex
        let mut patterns = Table::new();
        patterns.insert("user_id".to_string(), toml::Value::String(r"\d+".to_string()));
        let (matches, feedback) = validate_response_regex(&response, &patterns);
        assert!(matches, "Should match numeric regex: {:?}", feedback);

        // Test non-matching regex
        let mut patterns = Table::new();
        patterns.insert("token".to_string(), toml::Value::String("^[0-9]+$".to_string()));
        let (matches, feedback) = validate_response_regex(&response, &patterns);
        assert!(!matches, "Should not match numeric-only regex");
        assert!(feedback.is_some());
    }

    #[test]
    fn test_is_value_reference() {
        // Valid reference
        let valid_ref = serde_json::json!({
            "from": "test_login",
            "property": "response.token"
        });
        assert!(is_value_reference(&valid_ref));

        // Invalid - missing "from"
        let invalid1 = serde_json::json!({
            "property": "response.token"
        });
        assert!(!is_value_reference(&invalid1));

        // Invalid - missing "property"
        let invalid2 = serde_json::json!({
            "from": "test_login"
        });
        assert!(!is_value_reference(&invalid2));

        // Invalid - extra field
        let invalid3 = serde_json::json!({
            "from": "test_login",
            "property": "response.token",
            "extra": "field"
        });
        assert!(!is_value_reference(&invalid3));

        // Invalid - not an object
        let invalid4 = serde_json::json!("just a string");
        assert!(!is_value_reference(&invalid4));
    }

    #[test]
    fn test_resolve_payload_references() -> Result<()> {
        let mut responses = HashMap::new();
        responses.insert("test_login".to_string(), serde_json::json!({
            "access_token": "secret_token_123",
            "user": {
                "id": 42
            }
        }));

        // Test simple reference
        let mut payload = serde_json::json!({
            "auth_token": {
                "from": "test_login",
                "property": "access_token"
            }
        });
        resolve_payload_references(&mut payload, &responses)?;
        assert_eq!(payload["auth_token"], serde_json::json!("secret_token_123"));

        // Test nested reference
        let mut payload = serde_json::json!({
            "user_id": {
                "from": "test_login",
                "property": "user.id"
            }
        });
        resolve_payload_references(&mut payload, &responses)?;
        assert_eq!(payload["user_id"], serde_json::json!(42));

        // Test missing reference
        let mut payload = serde_json::json!({
            "token": {
                "from": "nonexistent",
                "property": "token"
            }
        });
        let result = resolve_payload_references(&mut payload, &responses);
        assert!(result.is_err());

        Ok(())
    }
}

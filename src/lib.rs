// Copyright (c) 2025 Jesse Lawson <jesse@lawsonry.com>
// GNU General Public License v3.0+ (see LICENSE or https://www.gnu.org/licenses/gpl-3.0.txt)

use anyhow::{Context, Result};
use clap::Parser;
use colored::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use toml::Table;

#[derive(Parser, Debug)]
#[command(author, version, about = format!("{}, the API server testing tool", "Tasty".bold()))]
pub struct Args {
    /// Base URL for the API (defaults to http://127.0.0.1:3030)
    #[arg(value_name = "URL")]
    pub base_url: Option<String>,

    /// Specific test files to run
    #[arg(value_name = "TESTS")]
    pub test_files: Vec<String>,

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
    pub payload: Table,

    /// The HTTP status code expected for a passing test
    pub expect_http_status: u16,

    /// Properties expected in the response object
    pub expect_response_includes: Option<Table>,

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

pub fn print_summary(stats: &TestStats) {
    println!("\nTest Summary:");
    println!("  Total tests: {}", stats.total);
    println!("  Passed: {}", stats.passed.to_string().green());
    println!("  Failed: {}", stats.failed.to_string().red());
    println!("  Skipped: {}", stats.skipped.to_string().yellow());
    println!("  Total duration: {}ms", stats.total_duration.as_millis());
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
            Err(e) => return Err(anyhow::anyhow!("Unable to access the folder '{}'\n  (Got \"{}\")\nBe sure the folder exists and is readable. If you don't understand why you are getting this error, try the help command:\n    {}", &tests_dir.display(), e, "tasty --help".blue().bold()))
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
) -> Result<()> {
    let start_time = Instant::now();
    let url = format!("{}/{}", base_url.trim_end_matches('/'), test.route);

    // Validate HTTP method
    if !METHODS.iter().any(|m| test.method.contains(m)) {
        test.outcome = Some(false);
        test.feedback = Some(format!("Invalid HTTP method: {}", test.method));
        return Ok(());
    }

    // Execute the request
    let response = match client
        .request(test.method.parse()?, &url)
        .json(&test.payload)
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

    let status_matches = response.status().as_u16() == test.expect_http_status;
    let actual_status = response.status().as_u16();

    // Parse and validate response
    let response_json: serde_json::Value = match response.json().await {
        Ok(json) => json,
        Err(e) => {
            test.outcome = Some(false);
            test.feedback = Some(format!("    HTTP Response: {}\n      Error: {}", 
                &response_status, e));
            return Ok(());
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

    // Now check our payload expectations:
    // Do all expected kv pairs exist in the response?
    // If no expectations were provided (None), consider it a match (true):
    let payload_matches = test
        .expect_response_includes
        .as_ref()
        .map(|expected| {
            // Verify that each expected key-value pair matches in the response
            expected
                .iter()
                .all(|(k, v)| {
                    if let Ok(json_v) = serde_json::to_value(v) {
                        response_json.get(k) == Some(&json_v)
                    } else {
                        false
                    }
                })
        })
        .unwrap_or(true);

    // Record test results
    test.duration = Some(start_time.elapsed());
    test.outcome = Some(status_matches && payload_matches);

    if !test.outcome.unwrap() {
        let mut feedback = Vec::new();

        if !status_matches {
            feedback.push(format!(
                "    {}\n    Expected: {}\n    Returned: {}",
                "Status code mismatch:".bold(),
                test.expect_http_status.to_string().dimmed(),
                actual_status.to_string().dimmed()
            ));
        }

        if !payload_matches {
            feedback.push(format!(
                "    {}\n    Expected: \n{}\n    Returned: \n{}",
                "Payload mismatch:".bold(),
                (serde_json::to_string_pretty(&test.expect_response_includes).unwrap())
                    .lines()
                    .map(|l| format!("      {}", l))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .dimmed(),
                (serde_json::to_string_pretty(&response_json).unwrap())
                    .lines()
                    .map(|l| format!("      {}", l))
                    .collect::<Vec<String>>()
                    .join("\n")
                    .dimmed()
            ));
        }

        test.feedback = Some(feedback.join("\n"));
    }

    Ok(())
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

        for (test_name, case) in test_cases {
            let mut test: TestCase = case.try_into()?;

            // Use the table key as the test name if none was provided
            if test.name.is_empty() {
                test.name = test_name;
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

            let result = if let Err(e) = run_test_case(&client, &base_url, &mut test, &args).await {
                println!("{}", "ERROR".red());
                println!("    {}", e);

                stats.failed += 1;
                TestResult {
                    name: test.name,
                    status: TestStatus::Error,
                    duration_ms: None,
                    feedback: Some(format!("    {}\n", e.to_string().dimmed().red())),
                }
            } else {
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
                                    format!("({}ms)", test.duration.unwrap().as_millis()).dimmed()
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

        assert!(
            files.contains(&PathBuf::from("examples/single_endpoint.toml")),
            "actual: {:?}",
            files
        );
        assert!(files.contains(&PathBuf::from("examples/login_tests.toml")));
        Ok(())
    }
}

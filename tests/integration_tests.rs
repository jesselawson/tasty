use anyhow::Result;
use httpmock::MockServer;
use std::path::PathBuf;
use tasty::{Args, run_tests};

#[tokio::test]
async fn test_examples() -> Result<()> {
    let server = MockServer::start();

    let args = Args {
        base_url: Some(server.base_url()),
        test_files: vec![
            "single_endpoint".to_string(),
            "login_tests".to_string(),
            "error_cases".to_string(),
        ],
        tests_folder: Some(PathBuf::from("examples")),
        timeout: 30,
        json: false,
        debug: false,
        headers: vec![],
    };

    server.mock(|when, then| {
        when.method("GET").path("/api/users/1");
        then.status(200).json_body_obj(&serde_json::json!({
            "id": 1,
            "name": "Test User"
        }));
    });

    server.mock(|when, then| {
        when.method("POST")
            .path("/auth/login")
            .json_body_obj(&serde_json::json!({
                "username": "test_user",
                "password": "correct_password"
            }));
        then.status(200).json_body_obj(&serde_json::json!({
            "token": "valid_token",
            "token_type": "Bearer"
        }));
    });

    server.mock(|when, then| {
        when.method("POST")
            .path("/auth/login")
            .json_body_obj(&serde_json::json!({
                "username": "test_user",
                "password": "wrong_password"
            }));
        then.status(401).json_body_obj(&serde_json::json!({
            "error": "Invalid credentials"
        }));
    });

    server.mock(|when, then| {
        when.method("GET").path("/api/timeout");
        then.delay(std::time::Duration::from_secs(2))
            .status(504)
            .json_body_obj(&serde_json::json!({
                "error": "Timeout"
            }));
    });

    server.mock(|when, then| {
        when.method("POST").path("/api/items");
        then.status(400).json_body_obj(&serde_json::json!({
            "error": "Invalid request body"
        }));
    });

    let result = run_tests(&args).await?;
    assert!(result.success);
    Ok(())
}

#[tokio::test]
async fn test_response_referencing() -> Result<()> {
    let server = MockServer::start();

    let args = Args {
        base_url: Some(server.base_url()),
        test_files: vec!["response_reference".to_string()],
        tests_folder: Some(PathBuf::from("examples")),
        timeout: 30,
        json: false,
        debug: false,
        headers: vec![],
    };

    // Mock for getting a token
    server.mock(|when, then| {
        when.method("POST")
            .path("/auth/token")
            .json_body_obj(&serde_json::json!({
                "client_id": "test_client"
            }));
        then.status(200).json_body_obj(&serde_json::json!({
            "access_token": "abc123xyz",
            "token_type": "Bearer",
            "expires_in": 3600
        }));
    });

    // Mock for protected endpoint that receives the token
    server.mock(|when, then| {
        when.method("GET")
            .path("/api/protected")
            .json_body_obj(&serde_json::json!({
                "auth_token": "abc123xyz"
            }));
        then.status(200).json_body_obj(&serde_json::json!({
            "status": "authorized",
            "user": "test_user"
        }));
    });

    let result = run_tests(&args).await?;
    assert!(result.success, "Response referencing test failed");
    Ok(())
}

#[tokio::test]
async fn test_regex_matching() -> Result<()> {
    let server = MockServer::start();

    let args = Args {
        base_url: Some(server.base_url()),
        test_files: vec!["regex_matching".to_string()],
        tests_folder: Some(PathBuf::from("examples")),
        timeout: 30,
        json: false,
        debug: false,
        headers: vec![],
    };

    // Mock for token endpoint
    server.mock(|when, then| {
        when.method("POST")
            .path("/auth/token")
            .json_body_obj(&serde_json::json!({
                "client_id": "test_client"
            }));
        then.status(200).json_body_obj(&serde_json::json!({
            "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
            "token_type": "Bearer",
            "expires_in": 3600
        }));
    });

    // Mock for user endpoint with regex-matchable response
    server.mock(|when, then| {
        when.method("GET").path("/api/users/42");
        then.status(200).json_body_obj(&serde_json::json!({
            "id": 42,
            "name": "Test User",
            "email": "test@example.com"
        }));
    });

    let result = run_tests(&args).await?;
    assert!(result.success, "Regex matching test failed");
    Ok(())
}

#[tokio::test]
async fn test_missing_reference_fails() -> Result<()> {
    let server = MockServer::start();

    // Create a temp directory with a test that references a non-existent test
    // Using a relative path under the project directory to work around get_test_files behavior
    let temp_dir = PathBuf::from("test_temp_missing_ref");
    std::fs::create_dir_all(&temp_dir)?;

    let test_content = r#"
[test_with_bad_reference]
method = "GET"
route = "api/test"
payload.token = { from = "nonexistent_test", property = "token" }
expect.http_status = 200
"#;
    std::fs::write(temp_dir.join("bad_ref.toml"), test_content)?;

    let args = Args {
        base_url: Some(server.base_url()),
        test_files: vec!["bad_ref".to_string()],
        tests_folder: Some(temp_dir.clone()),
        timeout: 30,
        json: false,
        debug: false,
        headers: vec![],
    };

    server.mock(|when, then| {
        when.method("GET").path("/api/test");
        then.status(200).json_body_obj(&serde_json::json!({
            "result": "ok"
        }));
    });

    let result = run_tests(&args).await?;

    // The test should fail because the reference doesn't exist
    assert!(!result.success, "Test with missing reference should fail");

    // Cleanup
    std::fs::remove_dir_all(&temp_dir)?;

    Ok(())
}

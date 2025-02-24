use anyhow::Result;
use httpmock::MockServer;
use std::path::PathBuf;
use tasty::{Args, run_tests};

#[tokio::test]
async fn test_examples() -> Result<()> {
    let server = MockServer::start();

    let args = Args {
        base_url: Some(server.base_url()),
        test_files: vec![],
        tests_folder: Some(PathBuf::from("examples")),
        timeout: 30,
        json: false,
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

//! The delete of the loopback client (model-delete), against the same real
//! loopback fake as the chat tests.

use std::time::Duration;

use super::tests::Fake;
use super::*;

async fn delete_to(fake: &Fake, host: &str, path: &str, name: &str) -> Result<(), String> {
    delete_with(host, fake.port, path, name).await
}

/// The status line and headers of a reply with an empty body.
fn reply(status: &str) -> Vec<u8> {
    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").into_bytes()
}

// Spec: exactly `DELETE /api/delete`, loopback only.
#[tokio::test]
async fn a_non_loopback_host_is_refused_and_never_connects() {
    let fake = Fake::start().await;
    for host in ["localhost", "0.0.0.0", "example.com"] {
        assert_eq!(
            delete_to(&fake, host, DELETE_PATH, "m").await,
            Err(DELETE_REFUSED.to_owned()),
            "{host}"
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fake.connections(), 0);
}

#[tokio::test]
async fn any_other_path_is_refused_and_never_connects() {
    let fake = Fake::start().await;
    for path in ["/api/pull", "/api/delete/", "/api/delete?x=1", "/api/tags"] {
        assert_eq!(
            delete_to(&fake, "127.0.0.1", path, "m").await,
            Err(DELETE_REFUSED.to_owned()),
            "{path}"
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fake.connections(), 0);
}

// Spec: the name goes in a JSON body, built by a serializer.
#[tokio::test]
async fn it_sends_delete_with_the_exact_json_body() {
    let mut fake = Fake::start().await;
    fake.send(&reply("200 OK"));
    fake.close();
    assert_eq!(
        delete_to(&fake, "127.0.0.1", DELETE_PATH, "qwen3:4b").await,
        Ok(())
    );
    let request = fake.request();
    assert!(
        request.starts_with("DELETE /api/delete HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(request.contains("\r\nHost: 127.0.0.1:"), "{request}");
    assert!(
        request.contains("\r\nContent-Type: application/json\r\n"),
        "{request}"
    );
    assert!(
        request.ends_with("\r\n\r\n{\"model\":\"qwen3:4b\"}"),
        "{request}"
    );
    assert!(request.contains("\r\nContent-Length: 20\r\n"), "{request}");
}

#[tokio::test]
async fn a_name_with_quotes_is_escaped_not_spliced() {
    let mut fake = Fake::start().await;
    fake.send(&reply("200 OK"));
    fake.close();
    let name = "we\"ird\\name";
    assert_eq!(
        delete_to(&fake, "127.0.0.1", DELETE_PATH, name).await,
        Ok(())
    );
    let body = fake.request().split("\r\n\r\n").nth(1).unwrap().to_owned();
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("the body is valid JSON");
    assert_eq!(parsed["model"], name);
    assert_eq!(parsed.as_object().unwrap().len(), 1);
}

// Spec: 200 is success; 404 is a failure saying the model was not found.
#[tokio::test]
async fn a_404_says_the_model_was_not_found() {
    let mut fake = Fake::start().await;
    fake.send(&reply("404 Not Found"));
    fake.close();
    assert_eq!(
        delete_to(&fake, "127.0.0.1", DELETE_PATH, "gone:1b").await,
        Err(DELETE_NOT_FOUND.to_owned())
    );
}

#[tokio::test]
async fn any_other_status_is_a_failure_that_names_it() {
    let mut fake = Fake::start().await;
    fake.send(&reply("500 Internal Server Error"));
    fake.close();
    assert_eq!(
        delete_to(&fake, "127.0.0.1", DELETE_PATH, "m").await,
        Err("Ollama answered HTTP 500".to_owned())
    );
}

#[tokio::test]
async fn nothing_listening_cannot_connect() {
    let refused = crate::runtime::refused_port::RefusedPort::new();
    assert_eq!(
        delete_with("127.0.0.1", refused.port(), DELETE_PATH, "m").await,
        Err(DELETE_CANNOT_CONNECT.to_owned())
    );
}

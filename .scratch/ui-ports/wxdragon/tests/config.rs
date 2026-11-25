//! The config file and restarting the server with new settings.

use std::{
    io::{Read as _, Write as _},
    net::TcpStream,
};

use question_desk::{
    config::{self, Config},
    desk::Desk,
    server::{ServerControl, ServerSettings},
};

#[test]
fn an_empty_file_gives_the_defaults() {
    let config = config::parse("").unwrap();
    assert_eq!(config, Config::default());
    assert_eq!(config.host, "127.0.0.1");
    assert_eq!(config.port, 7457);
}

#[test]
fn host_port_and_token_are_read() {
    let config = config::parse(
        r#"
        host = "0.0.0.0"
        port = 8000
        bearer_token = "s3cret"
        "#,
    )
    .unwrap();
    assert_eq!(
        config,
        Config {
            host: "0.0.0.0".into(),
            port: 8000,
            bearer_token: Some("s3cret".into()),
        }
    );
}

#[test]
fn invalid_settings_are_rejected() {
    for text in [
        r#"host = " ""#,
        "port = 0",
        r#"bearer_token = """#,
        r#"bearer_token = "two words""#,
        "unknown = 1",
    ] {
        assert!(config::parse(text).is_err(), "accepted {text}");
    }
}

#[test]
fn the_written_file_reads_back() {
    let config = Config {
        host: "localhost".into(),
        port: 9000,
        bearer_token: Some("s3cret".into()),
    };
    let text = toml::to_string(&config).unwrap();
    assert_eq!(config::parse(&text).unwrap(), config);
    let without_token = Config::default();
    let text = toml::to_string(&without_token).unwrap();
    assert!(!text.contains("bearer_token"), "{text}");
}

/// The HTTP status of a bare POST to `/mcp` on `port`.
fn post_status(port: u16, token: &str) -> u16 {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        stream,
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response[9..12].parse().unwrap()
}

fn settings(port: u16, token: &str) -> ServerSettings {
    ServerSettings {
        port,
        bearer_token: Some(token.into()),
        ..ServerSettings::default()
    }
}

fn port_of(url: &str) -> u16 {
    url.trim_end_matches("/mcp")
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn a_restart_applies_new_settings_on_the_same_port() {
    let mut server = ServerControl::new(Desk::default());
    let port = port_of(&server.restart(settings(0, "old")).unwrap());
    assert_eq!(post_status(port, "wrong"), 401);

    let url = server.restart(settings(port, "new")).unwrap();
    assert_eq!(port_of(&url), port);
    assert_eq!(post_status(port, "old"), 401);
    assert_ne!(post_status(port, "new"), 401);
}

#[test]
fn a_failed_restart_keeps_the_previous_server() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken_port = taken.local_addr().unwrap().port();
    let mut server = ServerControl::new(Desk::default());
    let port = port_of(&server.restart(settings(0, "old")).unwrap());

    let error = server.restart(settings(taken_port, "new")).unwrap_err();
    assert!(error.contains(&taken_port.to_string()), "{error}");
    assert!(server.is_running());
    assert_ne!(post_status(port, "old"), 401);
}

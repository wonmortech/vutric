//! R2 deploy kit: the template that `deploy/render-config.sh` fills, filled the same way, is a valid relay config
//! for both the systemd and the container layout, and the unit and compose files point at the same paths.

use vutric_relay::Config;

fn render(domain: &str, contact: &str, staging: bool, container: bool) -> String {
    let template = include_str!("../deploy/vutric-relay.toml.in");
    let (http, https, state) = if container {
        ("[::]:8080", "[::]:8443", "/data")
    } else {
        ("[::]:80", "[::]:443", "/var/lib/vutric-relay")
    };
    template
        .replace("@DOMAIN@", domain)
        .replace("@CONTACT@", contact)
        .replace("@STAGING@", if staging { "true" } else { "false" })
        .replace("@HTTP_BIND@", http)
        .replace("@HTTPS_BIND@", https)
        .replace("@STATE_DIR@", state)
}

#[test]
fn rendered_configs_parse() {
    for (staging, container) in [(false, false), (true, false), (false, true), (true, true)] {
        let text = render(
            "relay.example.org",
            "mailto:ops@example.org",
            staging,
            container,
        );
        for p in [
            "@DOMAIN@",
            "@CONTACT@",
            "@STAGING@",
            "@HTTP_BIND@",
            "@HTTPS_BIND@",
            "@STATE_DIR@",
        ] {
            assert!(!text.contains(p), "placeholder {p} left");
        }
        let c = Config::parse(&text).expect("valid config");
        assert_eq!(c.url(), "https://relay.example.org/");
        assert!(c.quic.is_some(), "QUIC address discovery is on");
        assert!(
            c.metrics_bind.is_some_and(|m| m.ip().is_loopback()),
            "metrics stay on localhost"
        );
        let port = if container { 8080 } else { 80 };
        assert_eq!(c.http.bind.port(), port);
    }
}

#[test]
fn unit_and_compose_agree_with_the_template() {
    let unit = include_str!("../deploy/vutric-relay.service");
    assert!(
        unit.contains("ExecStart=/usr/local/bin/vutric-relay /etc/vutric-relay/vutric-relay.toml")
    );
    assert!(
        unit.contains("StateDirectory=vutric-relay"),
        "ACME cache under /var/lib/vutric-relay"
    );
    assert!(unit.contains("DynamicUser=yes") && unit.contains("NoNewPrivileges=yes"));
    let compose = include_str!("../deploy/docker-compose.yml");
    for port in ["80:8080/tcp", "443:8443/tcp", "7842:7842/udp"] {
        assert!(compose.contains(port), "{port}");
    }
    assert!(compose.contains("/etc/vutric-relay/vutric-relay.toml:ro"));
    let script = include_str!("../deploy/render-config.sh");
    for p in [
        "@DOMAIN@",
        "@CONTACT@",
        "@STAGING@",
        "@HTTP_BIND@",
        "@HTTPS_BIND@",
        "@STATE_DIR@",
    ] {
        assert!(script.contains(p), "render-config.sh fills {p}");
    }
}

use reqwest::Url;
use std::net::Ipv4Addr;

const DEFAULT_PROFILE: &str = "local";
const DEFAULT_ACCOUNT_SERVER_URL: &str = "http://127.0.0.1:3010";
const CLOUD_ACCOUNT_SERVER_ORIGIN: &str = "https://api.stargathering.cn";

pub(crate) fn deployment_profile() -> &'static str {
    option_env!("POMEGRANATE_DEPLOYMENT_PROFILE").unwrap_or(DEFAULT_PROFILE)
}

pub(crate) fn account_server_origin() -> &'static str {
    option_env!("POMEGRANATE_ACCOUNT_SERVER_URL").unwrap_or(DEFAULT_ACCOUNT_SERVER_URL)
}

fn allow_insecure_public_ip_http() -> bool {
    option_env!("POMEGRANATE_ALLOW_INSECURE_PUBLIC_IP_HTTP") == Some("true")
}

fn has_explicit_port(value: &str) -> bool {
    let Some((_, remainder)) = value.split_once("://") else {
        return false;
    };
    let authority = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    let Some((host, port)) = authority.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && !port.is_empty()
        && port.chars().all(|character| character.is_ascii_digit())
        && port.parse::<u16>().is_ok_and(|value| value != 0)
}

fn is_public_ipv4(host: &str) -> bool {
    let Ok(address) = host.parse::<Ipv4Addr>() else {
        return false;
    };
    let [first, second, third, fourth] = address.octets();

    !(first == 0
        || first == 10
        || first == 127
        || (first == 100 && (64..=127).contains(&second))
        || (first == 169 && second == 254)
        || (first == 172 && (16..=31).contains(&second))
        || (first == 192 && second == 0 && third == 0)
        || (first == 192 && second == 0 && third == 2)
        || (first == 192 && second == 88 && third == 99)
        || (first == 192 && second == 168)
        || (first == 198 && (second == 18 || second == 19))
        || (first == 198 && second == 51 && third == 100)
        || (first == 203 && second == 0 && third == 113)
        || first >= 224
        || [first, second, third, fourth] == [255, 255, 255, 255])
}

fn validate_origin(
    profile: &str,
    value: &str,
    allow_insecure_http: bool,
) -> Result<(), &'static str> {
    let url = Url::parse(value).map_err(|_| "Account Server URL 无效")?;
    if url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Account Server URL 只能包含协议、主机和端口");
    }

    let host = url.host_str().ok_or("Account Server URL 缺少主机")?;
    let is_loopback = host == "127.0.0.1" || host == "localhost";
    let public_ip_test_is_valid = is_public_ipv4(host)
        && has_explicit_port(value)
        && (url.scheme() == "https" || (url.scheme() == "http" && allow_insecure_http));
    match profile {
        "local" if url.scheme() == "http" && is_loopback => Ok(()),
        "lan" if url.scheme() == "http" && !is_loopback && host != "0.0.0.0" => Ok(()),
        "cloud" if value == CLOUD_ACCOUNT_SERVER_ORIGIN => Ok(()),
        "public-ip-test" if public_ip_test_is_valid => Ok(()),
        "local" => Err("local 客户端必须连接 HTTP 回环地址"),
        "lan" => Err("lan 客户端必须连接可达的 HTTP 局域网地址"),
        "cloud" => Err("cloud 客户端必须连接指定的 HTTPS Account Server"),
        "public-ip-test" => {
            Err("public-ip-test 客户端必须连接带明确端口的公网 IPv4；HTTP 需要构建时显式允许")
        }
        _ => Err("未知的客户端部署 profile"),
    }
}

pub(crate) fn account_server_endpoint(path: &str) -> String {
    let profile = deployment_profile();
    let origin = account_server_origin();
    validate_origin(profile, origin, allow_insecure_public_ip_http())
        .expect("invalid embedded Account Server configuration");
    assert!(
        path.starts_with('/'),
        "Account Server endpoint path must be absolute"
    );
    format!("{origin}{path}")
}

#[cfg(test)]
mod tests {
    use super::{is_public_ipv4, validate_origin};
    use std::net::Ipv4Addr;

    fn public_ipv4_fixture() -> String {
        (1..224)
            .map(|first| Ipv4Addr::new(first, 1, 1, 1).to_string())
            .find(|candidate| is_public_ipv4(candidate))
            .expect("a syntactically public IPv4 fixture should exist")
    }

    #[test]
    fn accepts_local_lan_and_cloud_origins() {
        assert!(validate_origin("local", "http://127.0.0.1:3010", false).is_ok());
        assert!(validate_origin("lan", "http://192.168.31.210:3010", false).is_ok());
        assert!(validate_origin("cloud", "https://api.stargathering.cn", false).is_ok());
    }

    #[test]
    fn rejects_cross_profile_and_unsafe_origins() {
        assert!(validate_origin("local", "http://0.0.0.0:3010", false).is_err());
        assert!(validate_origin("lan", "http://127.0.0.1:3010", false).is_err());
        assert!(validate_origin("lan", "http://192.168.31.210:3010/path", false).is_err());
        for origin in [
            "",
            "http://api.stargathering.cn",
            "http://127.0.0.1:3010",
            "http://localhost:3010",
            "http://192.168.31.210:3010",
            "https://localhost:3010",
            "https://127.0.0.1:3010",
            "https://api.example.com",
            "https://user:password@api.stargathering.cn",
            "ftp://api.stargathering.cn",
            "https://api.stargathering.cn:443",
            "https://api.stargathering.cn/v1",
        ] {
            assert!(
                validate_origin("cloud", origin, false).is_err(),
                "cloud origin should have been rejected"
            );
        }
    }

    #[test]
    fn local_and_lan_profiles_keep_their_existing_rules() {
        assert!(validate_origin("local", "http://localhost:3010", false).is_ok());
        assert!(validate_origin("lan", "http://10.0.0.8:3010", false).is_ok());
        assert!(validate_origin("local", "https://api.stargathering.cn", false).is_err());
        assert!(validate_origin("lan", "https://api.stargathering.cn", false).is_err());
    }

    #[test]
    fn public_ip_test_requires_an_explicit_public_ipv4_and_port() {
        let public_ip = public_ipv4_fixture();
        let https_origin = format!("https://{public_ip}:49152");
        let http_origin = format!("http://{public_ip}:49153");
        assert!(validate_origin("public-ip-test", &https_origin, false).is_ok());
        assert!(validate_origin("public-ip-test", &http_origin, false).is_err());
        assert!(validate_origin("public-ip-test", &http_origin, true).is_ok());

        let rejected_origins = [
            String::new(),
            format!("https://{public_ip}"),
            "https://localhost:49152".to_string(),
            "https://127.0.0.1:49152".to_string(),
            "https://0.0.0.0:49152".to_string(),
            "https://10.0.0.8:49152".to_string(),
            "https://172.16.0.8:49152".to_string(),
            "https://192.168.1.8:49152".to_string(),
            "https://169.254.1.8:49152".to_string(),
            "https://192.0.2.8:49152".to_string(),
            "https://198.51.100.8:49152".to_string(),
            "https://203.0.113.8:49152".to_string(),
            "https://255.255.255.255:49152".to_string(),
            format!("ftp://{public_ip}:49152"),
            format!("file://{public_ip}:49152"),
            format!("https://user:password@{public_ip}:49152"),
            format!("https://{public_ip}:49152/v1"),
        ];
        for origin in rejected_origins {
            assert!(
                validate_origin("public-ip-test", &origin, true).is_err(),
                "public-ip-test origin should have been rejected: {origin}"
            );
        }
    }

    #[test]
    fn cloud_still_rejects_public_ip_and_http_origins() {
        let public_ip = public_ipv4_fixture();
        assert!(validate_origin("cloud", &format!("https://{public_ip}:49152"), false).is_err());
        assert!(validate_origin("cloud", &format!("http://{public_ip}:49153"), true).is_err());
    }
}

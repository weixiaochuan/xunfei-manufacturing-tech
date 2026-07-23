use reqwest::Url;

const DEFAULT_PROFILE: &str = "local";
const DEFAULT_ACCOUNT_SERVER_URL: &str = "http://127.0.0.1:3010";
const CLOUD_ACCOUNT_SERVER_ORIGIN: &str = "https://api.stargathering.cn";

pub(crate) fn deployment_profile() -> &'static str {
    option_env!("POMEGRANATE_DEPLOYMENT_PROFILE").unwrap_or(DEFAULT_PROFILE)
}

pub(crate) fn account_server_origin() -> &'static str {
    option_env!("POMEGRANATE_ACCOUNT_SERVER_URL").unwrap_or(DEFAULT_ACCOUNT_SERVER_URL)
}

fn validate_origin(profile: &str, value: &str) -> Result<(), &'static str> {
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
    match profile {
        "local" if url.scheme() == "http" && is_loopback => Ok(()),
        "lan" if url.scheme() == "http" && !is_loopback && host != "0.0.0.0" => Ok(()),
        "cloud" if value == CLOUD_ACCOUNT_SERVER_ORIGIN => Ok(()),
        "local" => Err("local 客户端必须连接 HTTP 回环地址"),
        "lan" => Err("lan 客户端必须连接可达的 HTTP 局域网地址"),
        "cloud" => Err("cloud 客户端必须连接指定的 HTTPS Account Server"),
        _ => Err("未知的客户端部署 profile"),
    }
}

pub(crate) fn account_server_endpoint(path: &str) -> String {
    let profile = deployment_profile();
    let origin = account_server_origin();
    validate_origin(profile, origin).expect("invalid embedded Account Server configuration");
    assert!(path.starts_with('/'), "Account Server endpoint path must be absolute");
    format!("{origin}{path}")
}

#[cfg(test)]
mod tests {
    use super::validate_origin;

    #[test]
    fn accepts_local_lan_and_cloud_origins() {
        assert!(validate_origin("local", "http://127.0.0.1:3010").is_ok());
        assert!(validate_origin("lan", "http://192.168.31.210:3010").is_ok());
        assert!(validate_origin("cloud", "https://api.stargathering.cn").is_ok());
    }

    #[test]
    fn rejects_cross_profile_and_unsafe_origins() {
        assert!(validate_origin("local", "http://0.0.0.0:3010").is_err());
        assert!(validate_origin("lan", "http://127.0.0.1:3010").is_err());
        assert!(validate_origin("lan", "http://192.168.31.210:3010/path").is_err());
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
                validate_origin("cloud", origin).is_err(),
                "cloud origin should have been rejected"
            );
        }
    }

    #[test]
    fn local_and_lan_profiles_keep_their_existing_rules() {
        assert!(validate_origin("local", "http://localhost:3010").is_ok());
        assert!(validate_origin("lan", "http://10.0.0.8:3010").is_ok());
        assert!(validate_origin("local", "https://api.stargathering.cn").is_err());
        assert!(validate_origin("lan", "https://api.stargathering.cn").is_err());
    }
}

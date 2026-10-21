use anyhow::Context;

pub struct Config {
    pub bind_addr: std::net::SocketAddr,
    pub database_url: String,
    /// The one time zone all dates are shown and entered in (DESIGN §9.1).
    pub display_tz: chrono_tz::Tz,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse()
            .context("BIND_ADDR is not a valid address")?;
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
        let display_tz = std::env::var("DISPLAY_TZ")
            .unwrap_or_else(|_| "UTC".to_string())
            .parse()
            .map_err(|e| anyhow::anyhow!("DISPLAY_TZ is not a valid time zone name: {e}"))?;
        Ok(Config {
            bind_addr,
            database_url,
            display_tz,
        })
    }
}

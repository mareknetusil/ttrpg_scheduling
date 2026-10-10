use anyhow::Context;

pub struct Config {
    pub bind_addr: std::net::SocketAddr,
    pub database_url: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = std::env::var("BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse()
            .context("BIND_ADDR is not a valid address")?;
        let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
        Ok(Config {
            bind_addr,
            database_url,
        })
    }
}

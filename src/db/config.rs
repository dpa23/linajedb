use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub struct MySqlConfig {
    pub user: String,
    pub password: Option<String>,
    pub host: String,
    pub port: u16,
}

impl MySqlConfig {
    pub fn from_my_cnf() -> Option<Self> {
        let home = std::env::var("HOME").ok()?;
        let path = Path::new(&home).join(".my.cnf");
        if !path.exists() {
            return None;
        }

        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);

        let mut user = String::from("root");
        let mut password = None;
        let mut host = String::from("127.0.0.1");
        let mut port = 3306;
        let mut in_client_section = false;

        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_client_section = trimmed == "[client]" || trimmed == "[mysql]";
                continue;
            }

            if in_client_section && trimmed.contains('=') {
                let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
                let key = parts[0].trim();
                let val = parts[1].trim().trim_matches('"').trim_matches('\'');

                match key {
                    "user" => user = val.to_string(),
                    "password" => password = Some(val.to_string()),
                    "host" => host = val.to_string(),
                    "port" => port = val.parse().unwrap_or(3306),
                    _ => {}
                }
            }
        }

        Some(Self { user, password, host, port })
    }

    pub fn to_connection_string(&self, database: &str) -> String {
        let pass = match &self.password {
            Some(p) => format!(":{}", p),
            None => "".to_string(),
        };
        format!(
            "mysql://{}{}@{}:{}/{}",
            self.user, pass, self.host, self.port, database
        )
    }
}

pub struct PgCredentials {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
}

impl PgCredentials {
    pub fn from_pgpass(target_host: &str, target_port: u16, target_db: &str, target_user: &str) -> Option<Self> {
        let home = std::env::var("HOME").ok()?;
        let path = Path::new(&home).join(".pgpass");
        if !path.exists() {
            return None;
        }

        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);

        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }

            let parts: Vec<&str> = trimmed.split(':').collect();
            if parts.len() != 5 {
                continue;
            }

            let host = parts[0];
            let port = parts[1];
            let db = parts[2];
            let user = parts[3];
            let pass = parts[4];

            let host_match = host == "*" || host == target_host;
            let port_match = port == "*" || port.parse::<u16>().ok() == Some(target_port);
            let db_match = db == "*" || db == target_db;
            let user_match = user == "*" || user == target_user;

            if host_match && port_match && db_match && user_match {
                return Some(Self {
                    host: if host == "*" { target_host.to_string() } else { host.to_string() },
                    port: if port == "*" { target_port } else { port.parse().unwrap_or(target_port) },
                    database: if db == "*" { target_db.to_string() } else { db.to_string() },
                    user: user.to_string(),
                    password: Some(pass.to_string()),
                });
            }
        }
        None
    }

    pub fn to_connection_string(&self) -> String {
        let pass = match &self.password {
            Some(p) => format!(":{}", p),
            None => "".to_string(),
        };
        format!(
            "postgres://{}{}@{}:{}/{}",
            self.user, pass, self.host, self.port, self.database
        )
    }
}

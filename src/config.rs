use std::env;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::optimizer::PackingConfig;

/// Complete application configuration, loaded from environment variables or default values.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub api: ApiConfig,
    pub update: UpdateConfig,
    pub optimizer: OptimizerConfig,
}

impl AppConfig {
    /// Creates a configuration from the currently available environment variables.
    pub fn from_env() -> Self {
        Self {
            api: ApiConfig::from_env(),
            update: UpdateConfig::from_env(),
            optimizer: OptimizerConfig::from_env(),
        }
    }
}

/// Configuration for the API server.
#[derive(Clone, Debug)]
pub struct ApiConfig {
    bind_ip: IpAddr,
    display_host: String,
    port: u16,
}

impl ApiConfig {
    const DEFAULT_HOST: &'static str = "0.0.0.0";
    const DEFAULT_PORT: u16 = 8080;

    fn from_env() -> Self {
        let host_value =
            env_string("SORT_IT_NOW_API_HOST").unwrap_or_else(|| Self::DEFAULT_HOST.to_string());
        let (bind_ip, effective_host) = match host_value.parse::<IpAddr>() {
            Ok(ip) => (ip, host_value),
            Err(err) => {
                eprintln!(
                    "⚠️ Could not parse SORT_IT_NOW_API_HOST ('{}'): {}. Using {}.",
                    host_value,
                    err,
                    Self::DEFAULT_HOST
                );
                (
                    Self::DEFAULT_HOST
                        .parse::<IpAddr>()
                        .expect("Default host must be valid"),
                    Self::DEFAULT_HOST.to_string(),
                )
            }
        };

        let port = match env_string("SORT_IT_NOW_API_PORT") {
            Some(raw) => match raw.parse::<u16>() {
                Ok(value) if value != 0 => value,
                Ok(_) => {
                    eprintln!(
                        "⚠️ SORT_IT_NOW_API_PORT must not be 0. Using {}.",
                        Self::DEFAULT_PORT
                    );
                    Self::DEFAULT_PORT
                }
                Err(err) => {
                    eprintln!(
                        "⚠️ Could not parse SORT_IT_NOW_API_PORT ('{}'): {}. Using {}.",
                        raw,
                        err,
                        Self::DEFAULT_PORT
                    );
                    Self::DEFAULT_PORT
                }
            },
            None => Self::DEFAULT_PORT,
        };

        Self {
            bind_ip,
            display_host: effective_host,
            port,
        }
    }

    /// Socket address to bind the server to.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.port)
    }

    /// Visible hostname for logging and hints.
    pub fn display_host(&self) -> &str {
        &self.display_host
    }

    /// Configured port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Indicates whether binding to all interfaces.
    pub fn binds_to_all_interfaces(&self) -> bool {
        match self.bind_ip {
            IpAddr::V4(addr) => addr == Ipv4Addr::UNSPECIFIED,
            IpAddr::V6(addr) => addr == Ipv6Addr::UNSPECIFIED,
        }
    }

    /// Checks whether the hostname matches the default value.
    pub fn uses_default_host(&self) -> bool {
        self.display_host == Self::DEFAULT_HOST
    }
}

/// Configuration for the updater.
#[derive(Clone, Debug)]
pub struct UpdateConfig {
    owner: String,
    repo: String,
}

impl UpdateConfig {
    const DEFAULT_OWNER: &'static str = "JosunLP";
    const DEFAULT_REPO: &'static str = "sort-it-now";

    fn from_env() -> Self {
        Self {
            owner: env_string("SORT_IT_NOW_GITHUB_OWNER")
                .unwrap_or_else(|| Self::DEFAULT_OWNER.to_string()),
            repo: env_string("SORT_IT_NOW_GITHUB_REPO")
                .unwrap_or_else(|| Self::DEFAULT_REPO.to_string()),
        }
    }

    /// GitHub owner (organization or user) from which releases originate.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// GitHub repository name from which releases are loaded.
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// Returns the URL where the latest release is queried.
    pub fn latest_release_endpoint(&self) -> String {
        format!(
            "https://api.github.com/repos/{owner}/{repo}/releases/latest",
            owner = self.owner(),
            repo = self.repo()
        )
    }
}

/// Configuration for heuristic pack optimization.
#[derive(Clone, Debug)]
pub struct OptimizerConfig {
    packing: PackingConfig,
}

impl OptimizerConfig {
    const GRID_STEP_VAR: &'static str = "SORT_IT_NOW_PACKING_GRID_STEP";
    const SUPPORT_RATIO_VAR: &'static str = "SORT_IT_NOW_PACKING_SUPPORT_RATIO";
    const HEIGHT_EPSILON_VAR: &'static str = "SORT_IT_NOW_PACKING_HEIGHT_EPSILON";
    const GENERAL_EPSILON_VAR: &'static str = "SORT_IT_NOW_PACKING_GENERAL_EPSILON";
    const BALANCE_RATIO_VAR: &'static str = "SORT_IT_NOW_PACKING_BALANCE_LIMIT_RATIO";
    const FOOTPRINT_TOLERANCE_VAR: &'static str = "SORT_IT_NOW_PACKING_FOOTPRINT_TOLERANCE";
    const ALLOW_ROTATION_VAR: &'static str = "SORT_IT_NOW_PACKING_ALLOW_ROTATIONS";

    fn from_env() -> Self {
        let grid_step = load_f64_with_warning(
            Self::GRID_STEP_VAR,
            PackingConfig::DEFAULT_GRID_STEP,
            |value| value > 0.0,
            "must be greater than 0",
            "Warning: Adjusted grid step size may affect packing stability",
        );

        let support_ratio = load_f64_with_warning(
            Self::SUPPORT_RATIO_VAR,
            PackingConfig::DEFAULT_SUPPORT_RATIO,
            |value| (0.0..=1.0).contains(&value),
            "must be between 0 and 1",
            "Warning: Adjusted minimum support may lead to unstable stacks",
        );

        let height_epsilon = load_f64_with_warning(
            Self::HEIGHT_EPSILON_VAR,
            PackingConfig::DEFAULT_HEIGHT_EPSILON,
            |value| value > 0.0,
            "must be greater than 0",
            "Warning: Adjusted height tolerance may cause unexpected placements",
        );

        let general_epsilon = load_f64_with_warning(
            Self::GENERAL_EPSILON_VAR,
            PackingConfig::DEFAULT_GENERAL_EPSILON,
            |value| value > 0.0,
            "must be greater than 0",
            "Warning: Adjusted tolerances may cause numerical instabilities",
        );

        let balance_limit_ratio = load_f64_with_warning(
            Self::BALANCE_RATIO_VAR,
            PackingConfig::DEFAULT_BALANCE_LIMIT_RATIO,
            |value| (0.0..=1.0).contains(&value),
            "must be between 0 and 1",
            "Warning: Adjusted balance limits may cause stacks to tip over",
        );

        let footprint_cluster_tolerance = load_f64_with_warning(
            Self::FOOTPRINT_TOLERANCE_VAR,
            PackingConfig::DEFAULT_FOOTPRINT_CLUSTER_TOLERANCE,
            // Values above 0.5 would group excessively dissimilar footprints, defeating the clustering purpose.
            |value| (0.0..=0.5).contains(&value),
            "must be between 0 and 0.5",
            "Warning: Adjusted footprint grouping may lead to unexpected placements",
        );

        let allow_item_rotation = env_string(Self::ALLOW_ROTATION_VAR)
            .and_then(|raw| parse_bool(&raw, Self::ALLOW_ROTATION_VAR))
            .unwrap_or(PackingConfig::DEFAULT_ALLOW_ITEM_ROTATION);

        let packing = PackingConfig::builder()
            .grid_step(grid_step)
            .support_ratio(support_ratio)
            .height_epsilon(height_epsilon)
            .general_epsilon(general_epsilon)
            .balance_limit_ratio(balance_limit_ratio)
            .footprint_cluster_tolerance(footprint_cluster_tolerance)
            .allow_item_rotation(allow_item_rotation)
            .build();

        Self { packing }
    }

    /// Returns the configured PackingConfig.
    pub fn packing_config(&self) -> PackingConfig {
        self.packing
    }
}

fn env_string(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        }
        Err(env::VarError::NotPresent) => None,
        Err(err) => {
            eprintln!(
                "⚠️ Access to {} failed: {}. Using default value.",
                name, err
            );
            None
        }
    }
}

fn parse_bool(raw: &str, var_name: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "on" => Some(true),
        "0" | "false" | "no" | "n" | "off" => Some(false),
        other => {
            eprintln!(
                "⚠️ Could not interpret {} ('{}') as boolean value. Using default value.",
                var_name, other
            );
            None
        }
    }
}

fn load_f64_with_warning(
    var_name: &str,
    default: f64,
    validator: impl Fn(f64) -> bool,
    invalid_hint: &str,
    warning: &str,
) -> f64 {
    match env_string(var_name) {
        Some(raw) => match raw.parse::<f64>() {
            Ok(value) => {
                if !validator(value) {
                    eprintln!(
                        "⚠️ {} contains invalid value '{}': {}. Using {}.",
                        var_name, raw, invalid_hint, default
                    );
                    default
                } else {
                    let tolerance = (default.abs().max(1.0)) * 1e-9;
                    if (value - default).abs() > tolerance {
                        println!("⚠️ {} ({} = {}).", warning, var_name, value);
                    }
                    value
                }
            }
            Err(err) => {
                eprintln!(
                    "⚠️ Could not parse {} ('{}') as number: {}. Using {}.",
                    var_name, raw, err, default
                );
                default
            }
        },
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool_true_values() {
        assert_eq!(parse_bool("1", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("true", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("yes", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("y", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("on", "TEST_VAR"), Some(true));

        // Test case insensitivity
        assert_eq!(parse_bool("TRUE", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("Yes", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("ON", "TEST_VAR"), Some(true));

        // Test with whitespace
        assert_eq!(parse_bool(" true ", "TEST_VAR"), Some(true));
        assert_eq!(parse_bool("  1  ", "TEST_VAR"), Some(true));
    }

    #[test]
    fn test_parse_bool_false_values() {
        assert_eq!(parse_bool("0", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("false", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("no", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("n", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("off", "TEST_VAR"), Some(false));

        // Test case insensitivity
        assert_eq!(parse_bool("FALSE", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("No", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("OFF", "TEST_VAR"), Some(false));

        // Test with whitespace
        assert_eq!(parse_bool(" false ", "TEST_VAR"), Some(false));
        assert_eq!(parse_bool("  0  ", "TEST_VAR"), Some(false));
    }

    #[test]
    fn test_parse_bool_invalid_values() {
        assert_eq!(parse_bool("invalid", "TEST_VAR"), None);
        assert_eq!(parse_bool("2", "TEST_VAR"), None);
        assert_eq!(parse_bool("maybe", "TEST_VAR"), None);
        assert_eq!(parse_bool("", "TEST_VAR"), None);
        assert_eq!(parse_bool("  ", "TEST_VAR"), None);
    }
}

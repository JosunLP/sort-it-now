use std::env;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::optimizer::PackingConfig;

/// Gesamte Anwendungskonfiguration, geladen aus Umgebungsvariablen oder Defaultwerten.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub api: ApiConfig,
    pub update: UpdateConfig,
    pub optimizer: OptimizerConfig,
}

impl AppConfig {
    /// Erstellt eine Konfiguration aus den aktuell verfügbaren Umgebungsvariablen.
    pub fn from_env() -> Self {
        Self {
            api: ApiConfig::from_env(),
            update: UpdateConfig::from_env(),
            optimizer: OptimizerConfig::from_env(),
        }
    }
}

/// Konfiguration für den API-Server.
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
                    "⚠️ Konnte SORT_IT_NOW_API_HOST ('{}') nicht parsen: {}. Verwende {}.",
                    host_value,
                    err,
                    Self::DEFAULT_HOST
                );
                (
                    Self::DEFAULT_HOST
                        .parse::<IpAddr>()
                        .expect("Default-Host muss gültig sein"),
                    Self::DEFAULT_HOST.to_string(),
                )
            }
        };

        let port = match env_string("SORT_IT_NOW_API_PORT") {
            Some(raw) => match raw.parse::<u16>() {
                Ok(value) if value != 0 => value,
                Ok(_) => {
                    eprintln!(
                        "⚠️ SORT_IT_NOW_API_PORT darf nicht 0 sein. Verwende {}.",
                        Self::DEFAULT_PORT
                    );
                    Self::DEFAULT_PORT
                }
                Err(err) => {
                    eprintln!(
                        "⚠️ Konnte SORT_IT_NOW_API_PORT ('{}') nicht parsen: {}. Verwende {}.",
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

    /// Socket-Adresse, an die der Server gebunden werden soll.
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.bind_ip, self.port)
    }

    /// Sichtbarer Hostname für Logging und Hinweise.
    pub fn display_host(&self) -> &str {
        &self.display_host
    }

    /// Konfigurierter Port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Gibt an, ob auf alle Interfaces gebunden wird.
    pub fn binds_to_all_interfaces(&self) -> bool {
        match self.bind_ip {
            IpAddr::V4(addr) => addr == Ipv4Addr::UNSPECIFIED,
            IpAddr::V6(addr) => addr == Ipv6Addr::UNSPECIFIED,
        }
    }

    /// Prüft, ob der Hostname dem Standardwert entspricht.
    pub fn uses_default_host(&self) -> bool {
        self.display_host == Self::DEFAULT_HOST
    }
}

/// Konfiguration für den Updater.
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

    /// GitHub Owner (Organisation oder Benutzer), von dem die Releases stammen.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// GitHub Repositoryname, von dem Releases geladen werden.
    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// Liefert die URL, unter der das aktuellste Release abgefragt wird.
    pub fn latest_release_endpoint(&self) -> String {
        format!(
            "https://api.github.com/repos/{owner}/{repo}/releases/latest",
            owner = self.owner(),
            repo = self.repo()
        )
    }
}

/// Konfiguration für die heuristische Pack-Optimierung.
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

    fn from_env() -> Self {
        let grid_step = load_f64_with_warning(
            Self::GRID_STEP_VAR,
            PackingConfig::DEFAULT_GRID_STEP,
            |value| value > 0.0,
            "muss größer als 0 sein",
            "Warnung: Angepasste Raster-Schrittweite kann die Pack-Stabilität beeinträchtigen",
        );

        let support_ratio = load_f64_with_warning(
            Self::SUPPORT_RATIO_VAR,
            PackingConfig::DEFAULT_SUPPORT_RATIO,
            |value| (0.0..=1.0).contains(&value),
            "muss zwischen 0 und 1 liegen",
            "Warnung: Angepasste Mindestauflage kann zu instabilen Stapeln führen",
        );

        let height_epsilon = load_f64_with_warning(
            Self::HEIGHT_EPSILON_VAR,
            PackingConfig::DEFAULT_HEIGHT_EPSILON,
            |value| value > 0.0,
            "muss größer als 0 sein",
            "Warnung: Angepasste Höhen-Toleranz kann unerwartete Platzierungen verursachen",
        );

        let general_epsilon = load_f64_with_warning(
            Self::GENERAL_EPSILON_VAR,
            PackingConfig::DEFAULT_GENERAL_EPSILON,
            |value| value > 0.0,
            "muss größer als 0 sein",
            "Warnung: Angepasste Toleranzen können numerische Instabilitäten hervorrufen",
        );

        let balance_limit_ratio = load_f64_with_warning(
            Self::BALANCE_RATIO_VAR,
            PackingConfig::DEFAULT_BALANCE_LIMIT_RATIO,
            |value| (0.0..=1.0).contains(&value),
            "muss zwischen 0 und 1 liegen",
            "Warnung: Angepasste Balance-Grenzen können zum Umkippen von Stapeln führen",
        );

        let footprint_cluster_tolerance = load_f64_with_warning(
            Self::FOOTPRINT_TOLERANCE_VAR,
            PackingConfig::DEFAULT_FOOTPRINT_CLUSTER_TOLERANCE,
            |value| (0.0..=0.5).contains(&value),
            "muss zwischen 0 und 0.5 liegen",
            "Warnung: Angepasste Footprint-Gruppierung kann zu unerwarteten Platzierungen führen",
        );

        let packing = PackingConfig::builder()
            .grid_step(grid_step)
            .support_ratio(support_ratio)
            .height_epsilon(height_epsilon)
            .general_epsilon(general_epsilon)
            .balance_limit_ratio(balance_limit_ratio)
            .footprint_cluster_tolerance(footprint_cluster_tolerance)
            .build();

        Self { packing }
    }

    /// Liefert die konfigurierte PackingConfig.
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
                "⚠️ Zugriff auf {} fehlgeschlagen: {}. Verwende Standardwert.",
                name, err
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
                        "⚠️ {} enthält ungültigen Wert '{}': {}. Verwende {}.",
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
                    "⚠️ Konnte {} ('{}') nicht als Zahl parsen: {}. Verwende {}.",
                    var_name, raw, err, default
                );
                default
            }
        },
        None => default,
    }
}

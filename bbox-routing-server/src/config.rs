use bbox_common::config::from_config_opt_or_exit;
use serde::Deserialize;

#[derive(Deserialize, Default, Debug)]
#[serde(default, deny_unknown_fields)]
pub struct RoutingServerCfg {
    pub service: Vec<RoutingServiceCfg>,
}

/// Routing service configuration
#[derive(Deserialize, Debug)]
pub struct RoutingServiceCfg {
    pub profile: Option<String>,
    pub gpkg: String,
    pub table: String,
    pub geom: String,
}

impl RoutingServerCfg {
    pub fn from_config() -> Option<Self> {
        from_config_opt_or_exit("routing")
    }
}

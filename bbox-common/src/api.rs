use crate::ogcapi::*;

#[derive(Clone)]
pub struct OgcApiInventory {
    pub landing_page_links: Vec<ApiLink>,
    pub conformance_classes: Vec<String>,
    pub collections: Vec<CoreCollection>,
}

impl OgcApiInventory {
    pub fn new() -> Self {
        OgcApiInventory {
            landing_page_links: Vec::new(),
            conformance_classes: Vec::new(),
            collections: Vec::new(),
        }
    }
}

/// OpenAPi doc collection
#[derive(Clone)]
pub struct OpenApiDoc(serde_yaml::Value);

impl OpenApiDoc {
    pub fn new() -> Self {
        Self::from_yaml("{}", "")
    }
    pub fn from_yaml(yaml: &str, _prefix: &str) -> Self {
        OpenApiDoc(serde_yaml::from_str(yaml).unwrap())
    }
    pub fn extend(&mut self, yaml: &str, _prefix: &str) {
        let rhs_yaml = serde_yaml::from_str(yaml).unwrap();
        merge_level(&mut self.0, &rhs_yaml, "paths");
        if let Some(rhs_components) = rhs_yaml.get("components") {
            if let Some(components) = self.0.get_mut("components") {
                // merge 1st level children ("parameters", "responses", "schemas")
                for (key, _val) in rhs_components.as_mapping().unwrap().iter() {
                    merge_level(components, &rhs_components, key.as_str().unwrap());
                }
            } else {
                self.0
                    .as_mapping_mut()
                    .unwrap()
                    .insert("components".into(), rhs_components.clone());
            }
        }
    }
    /// Set url of first server entry
    pub fn set_server_url(&mut self, url: &str) {
        if let Some(servers) = self.0.get_mut("servers") {
            if let Some(server) = servers.get_mut(0) {
                if let Some(server) = server.as_mapping_mut() {
                    server[&"url".to_string().into()] = url.to_string().into();
                }
            }
        }
    }
    pub fn as_yaml(&self, public_base_url: &str) -> String {
        let mut doc = self.clone();
        doc.set_server_url(public_base_url);
        serde_yaml::to_string(&doc.0).unwrap()
    }
    pub fn as_json(&self, public_base_url: &str) -> serde_json::Value {
        let mut doc = self.clone();
        doc.set_server_url(public_base_url);
        serde_yaml::from_value::<serde_json::Value>(doc.0).unwrap()
    }
}

fn merge_level(yaml: &mut serde_yaml::Value, rhs_yaml: &serde_yaml::Value, key: &str) {
    if let Some(rhs_elem) = rhs_yaml.get(key) {
        if let Some(elem) = yaml.get_mut(key) {
            elem.as_mapping_mut()
                .unwrap()
                .extend(rhs_elem.as_mapping().unwrap().clone().into_iter());
        } else {
            yaml.as_mapping_mut()
                .unwrap()
                .insert(key.into(), rhs_elem.clone());
        }
    }
}

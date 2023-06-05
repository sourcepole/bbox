use crate::config::*;
use crate::service::TileService;
use crate::tilesource::{wms_fcgi::WmsMetrics, TileRead, TileResponse, TileSourceError};
use async_trait::async_trait;
use bbox_common::config::error_exit;
use log::info;
use martin_mbtiles::MbtilesPool;
use std::io::Cursor;
use tile_grid::{BoundingBox, Tile};

#[derive(Clone, Debug)]
pub struct MbtilesSource {
    mbt: MbtilesPool,
}

impl MbtilesSource {
    pub async fn from_config(cfg: &MbtilesSourceParamsCfg) -> Self {
        let mbt = MbtilesPool::new(cfg.path.clone())
            .await
            .unwrap_or_else(error_exit);
        //let opt = SqliteConnectOptions::new().filename(file).read_only(true);
        if let Ok(meta) = mbt.get_metadata().await {
            let tilejson = serde_json::to_string_pretty(&meta.tilejson).unwrap();
            info!("{tilejson}");
        }
        MbtilesSource { mbt }
    }
}

#[async_trait]
impl TileRead for MbtilesSource {
    async fn read_tile(
        &self,
        _service: &TileService,
        _extent: &BoundingBox,
    ) -> Result<TileResponse, TileSourceError> {
        unimplemented!()
    }

    async fn tile_request(
        &self,
        service: &TileService,
        extent: &BoundingBox,
        _crs: i32,
        _format: &str,
        _scheme: &str,
        _host: &str,
        _req_path: &str,
        _metrics: &WmsMetrics,
    ) -> Result<TileResponse, TileSourceError> {
        self.read_tile(service, extent).await
    }

    async fn xyz_request(
        &self,
        _service: &TileService,
        _tms_id: &str,
        tile: &Tile,
        _format: &str,
        _scheme: &str,
        _host: &str,
        _req_path: &str,
        _metrics: &WmsMetrics,
    ) -> Result<TileResponse, TileSourceError> {
        if let Some(content) = self
            .mbt
            .get_tile(tile.z, tile.x as u32, tile.y as u32)
            .await?
        {
            let content_type = Some("application/x-protobuf".to_string());
            let body = Box::new(Cursor::new(content));
            Ok(TileResponse {
                content_type,
                headers: TileResponse::new_headers(),
                body,
            })
        } else {
            todo!()
        }
    }
}

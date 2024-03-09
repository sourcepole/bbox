use crate::config::MbtilesStoreCfg;
use crate::mbtiles_ds::{Error as MbtilesDsError, MbtilesDatasource};
use crate::store::{BoxRead, TileReader, TileStoreError, TileWriter};
use async_trait::async_trait;
use bbox_core::endpoints::TileResponse;
use flate2::{read::GzEncoder, Compression};
use log::info;
use martin_mbtiles::{CopyDuplicateMode, MbtType, Metadata};
use std::ffi::OsStr;
use std::io::{Cursor, Read};
use std::path::Path;
use tile_grid::Xyz;

#[derive(Clone, Debug)]
pub struct MbtilesStore {
    pub(crate) mbt: MbtilesDatasource,
}

impl MbtilesStore {
    pub async fn from_config(cfg: &MbtilesStoreCfg) -> Result<Self, MbtilesDsError> {
        info!("Creating connection pool for {}", &cfg.path.display());
        let mbt = MbtilesDatasource::from_config(cfg, None).await?;
        //let opt = SqliteConnectOptions::new().filename(file).read_only(true);
        Ok(MbtilesStore { mbt })
    }
    pub async fn from_config_writable(
        cfg: &MbtilesStoreCfg,
        metadata: Metadata,
    ) -> Result<Self, MbtilesDsError> {
        info!("Creating connection pool for {}", &cfg.path.display());
        let mbt = MbtilesDatasource::from_config(cfg, Some(metadata)).await?;
        Ok(MbtilesStore { mbt })
    }
    pub fn config_from_cli_arg(file_or_url: &str) -> Option<MbtilesStoreCfg> {
        match Path::new(file_or_url).extension().and_then(OsStr::to_str) {
            Some("mbtiles") => {
                let cfg = MbtilesStoreCfg {
                    path: file_or_url.into(),
                };
                Some(cfg)
            }
            _ => None,
        }
    }
}

#[async_trait]
impl TileWriter for MbtilesStore {
    async fn exists(&self, tile: &Xyz) -> bool {
        match self
            .mbt
            .get_tile(tile.z, tile.x as u32, tile.y as u32)
            .await
        {
            Ok(None) | Err(_) => false,
            Ok(_) => true,
        }
    }
    async fn put_tile(&self, tile: &Xyz, mut input: BoxRead) -> Result<(), TileStoreError> {
        let mut bytes: Vec<u8> = Vec::new();
        if let Some(martin_tile_utils::Format::Mvt) = self.mbt.format {
            let mut gz = GzEncoder::new(&mut *input, Compression::fast());
            gz.read_to_end(&mut bytes)?;
        } else {
            input.read_to_end(&mut bytes)?;
        }
        let mut conn = self.mbt.pool.acquire().await?;
        self.mbt
            .mbtiles
            .insert_tiles(
                &mut conn,
                MbtType::Flat,
                //MbtType::Normalized { hash_view: false }, -> "no such function: md5_hex"
                CopyDuplicateMode::Override,
                &[(tile.z, tile.x as u32, tile.y as u32, bytes)],
            )
            .await?;
        Ok(())
    }
    async fn put_tiles(&mut self, mut tiles: Vec<(Xyz, BoxRead)>) -> Result<(), TileStoreError> {
        let batch = tiles
            .iter_mut()
            .map(|(xyz, tile)| {
                let mut bytes: Vec<u8> = Vec::new();
                if let Some(martin_tile_utils::Format::Mvt) = self.mbt.format {
                    let mut gz = GzEncoder::new(&mut *tile, Compression::fast());
                    gz.read_to_end(&mut bytes).unwrap();
                } else {
                    tile.read_to_end(&mut bytes).unwrap();
                }
                (xyz.z, xyz.x as u32, xyz.y as u32, bytes)
            })
            .collect::<Vec<_>>();
        let mut conn = self.mbt.pool.acquire().await?;
        self.mbt
            .mbtiles
            .insert_tiles(
                &mut conn,
                MbtType::Flat,
                CopyDuplicateMode::Override,
                &batch,
            )
            .await?;
        Ok(())
    }
}

#[async_trait]
impl TileReader for MbtilesStore {
    async fn get_tile(&self, tile: &Xyz) -> Result<Option<TileResponse>, TileStoreError> {
        let resp = if let Some(content) = self
            .mbt
            .get_tile(tile.z, tile.x as u32, tile.y as u32)
            .await?
        {
            let content_type = Some("application/x-protobuf".to_string());
            let body = Box::new(Cursor::new(content));
            Some(TileResponse {
                content_type,
                headers: TileResponse::new_headers(),
                body,
            })
        } else {
            None
        };
        Ok(resp)
    }
}

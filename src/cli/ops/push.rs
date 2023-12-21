use std::fs::File;
use std::io::Cursor;
use std::path::PathBuf;
use std::str::FromStr;

use super::utils::{load_dor_store, save_dor_store, save_root_cid, load_root_cid};
use cid::Cid;

use crate::cli::config::{Config, ConfigError};
use crate::root_cid::{EthClient, EthClientError};
use crate::ipfs::{add_file_request, IpfsApi, IpfsClient, IpfsClientError, IpfsError};

pub async fn push(config: &Config, working_dir: PathBuf) -> Result<(), PushError> {
    let root_cid = load_root_cid(working_dir.clone())?; 
    
    let remote_ipfs_client = match config.ipfs_remote() {
        Some(ipfs_remote) => IpfsClient::try_from(ipfs_remote.clone())?,
        None => {
            return Err(PushError::MissingIpfsRemote);
        }
    };
    let mut eth_client = match config.eth_remote() {
        Some(eth_remote) => EthClient::try_from(eth_remote.clone())?,
        None => {
            return Err(PushError::MissingEthRemote);
        }
    };
    let local_wallet = config.local_wallet()?;
    let eth_client = eth_client.with_wallet_as_signer(local_wallet)?;

    let mut dor_store = load_dor_store(working_dir.clone())?;
    // let root_cid = load_root_cid(working_dir.clone())?;
    let objects = dor_store.objects();

    // Tell the remote to pin all the objects
    for (path, object) in objects.iter() {
        if block_exists(object.cid(), &remote_ipfs_client).await? {
            continue;
        }
        let cid = add_file(&working_dir.join(path), &remote_ipfs_client).await?;
        if cid != *object.cid() {
            return Err(PushError::CidMismatch(cid, object.cid().clone()));
        }
    }

    // Push our linking blocks to the remote, get the new root cid
    dor_store.set_previous_root(root_cid);
    // TODO: standardize passing around the dor_store accross the ipfs boundary
    let dor_store_vec = serde_json::to_vec(&dor_store)?;
    let dor_store_data = Cursor::new(dor_store_vec);
    let add_response = remote_ipfs_client
        .add_with_options(dor_store_data, add_file_request())
        .await?;
    let new_root_cid = Cid::from_str(&add_response.hash)?;

    // Push the new root cid to the eth client
    eth_client.update(root_cid, new_root_cid.clone()).await?;

    save_root_cid(working_dir.clone(), &new_root_cid)?;
    save_dor_store(working_dir.clone(), &dor_store)?;
    Ok(())
}

/// Add a file to the local ipfs node using its path
async fn add_file(path: &PathBuf, remote_ipfs_client: &IpfsClient) -> Result<Cid, PushError> {
    let file = File::open(path)?;
    let add_response = remote_ipfs_client
        .add_with_options(file, add_file_request())
        .await?;
    let cid = Cid::try_from(add_response.hash)?;
    Ok(cid)
}

/// Stat the cid on the remote ipfs node
/// Returns true if the cid exists on the remote ipfs node
async fn block_exists(cid: &Cid, remote_ipfs_client: &IpfsClient) -> Result<bool, PushError> {
    let cid = cid.to_string();
    let stat_response = remote_ipfs_client.block_stat(&cid);
    match stat_response.await {
        Ok(_) => Ok(true),
        Err(IpfsClientError::Api(api_error)) => {
            if api_error.code == 0 && api_error.message == "blockservice: key not found" {
                Ok(false)
            } else {
                Err(PushError::IpfsBackend(api_error.into()))
            }
        }
        Err(e) => Err(PushError::IpfsBackend(e)),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("config error")]
    Config(#[from] ConfigError),
    #[error("cid error: {0}")]
    Cid(#[from] cid::Error),
    #[error("cid mismatch: {0} != {1}")]
    CidMismatch(Cid, Cid),
    #[error("eth client error: {0}")]
    EthClient(#[from] EthClientError),
    #[error("fs-tree error: {0}")]
    FsTree(#[from] fs_tree::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse diff: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("could not strip prefix: {0}")]
    PathPrefix(#[from] std::path::StripPrefixError),
    #[error("ipfs error: {0}")]
    Ipfs(#[from] IpfsError),
    #[error("ipfs backend error: {0}")]
    IpfsBackend(#[from] ipfs_api_backend_hyper::Error),
    #[error("missing root cid")]
    MissingRootCid,
    #[error("missing ipfs remote")]
    MissingIpfsRemote,
    #[error("missing eth remote")]
    MissingEthRemote,
}

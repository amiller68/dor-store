use std::convert::TryFrom;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;

use cid::Cid;
use http::uri::Scheme;
use ipfs_api_backend_hyper::{IpfsClient as HyperIpfsClient, TryFromUri};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use url::Url;

pub use ipfs_api_backend_hyper::request::Add as AddRequest;
pub use ipfs_api_backend_hyper::IpfsApi;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A connection to an IPFS remote
pub struct IpfsRemote {
    /// Url pointing to an IPFS api
    /// Must include valid authentication if required
    pub api_url: Url,
    /// Url pointing to an IPFS gateway
    pub gateway_url: Url,
}

impl Default for IpfsRemote {
    fn default() -> Self {
        // Just use the default kubo configuration
        Self {
            api_url: Url::parse("http://127.0.0.1:5001").unwrap(),
            gateway_url: Url::parse("http://127.0.0.1:8080").unwrap(),
        }
    }
}

/// A wrapper around a gateway url
pub struct IpfsGateway(Url);

impl Default for IpfsGateway {
    fn default() -> Self {
        Self(Url::parse("http://127.0.0.1:8080").unwrap())
    }
}

impl From<IpfsRemote> for IpfsGateway {
    fn from(remote: IpfsRemote) -> Self {
        Self(remote.gateway_url.clone())
    }
}

impl IpfsGateway {
    pub async fn get(&self, cid: &Cid, path: Option<PathBuf>) -> Result<Vec<u8>, IpfsError> {
        let url = match path {
            Some(p) => Url::parse(&format!("{}.ipfs.{}/{}", cid, self.0, p.display())),
            None => Url::parse(&format!("{}.ipfs.{}", cid, self.0)),
        }?;
        let client = Client::builder().build()?;
        let resp = client.get(url).send().await?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }
}

#[derive(Default)]
pub struct IpfsClient(HyperIpfsClient);

impl TryFrom<IpfsRemote> for IpfsClient {
    type Error = IpfsError;

    fn try_from(remote: IpfsRemote) -> Result<Self, IpfsError> {
        let url = remote.api_url.clone();
        let scheme = Scheme::try_from(url.scheme())?;
        let username = url.username();
        let maybe_password = url.password();
        let host_str = url.host_str().unwrap();
        let port = url.port().unwrap_or(5001);
        let client = match maybe_password {
            Some(password) => HyperIpfsClient::from_host_and_port(scheme, host_str, port)?
                .with_credentials(username, password),
            None => HyperIpfsClient::from_host_and_port(scheme, host_str, port)?,
        };
        Ok(Self(client))
    }
}

impl Deref for IpfsClient {
    type Target = HyperIpfsClient;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn hash_file_request() -> AddRequest<'static> {
    let mut add = AddRequest::default();
    add.pin = Some(false);
    add.cid_version = Some(1);
    add.only_hash = Some(true);
    add.hash = Some("blake3");
    add
}

pub fn add_file_request() -> AddRequest<'static> {
    let mut add = AddRequest::default();
    add.pin = Some(true);
    add.cid_version = Some(1);
    add.hash = Some("blake3");
    add
}

pub type IpfsClientError = ipfs_api_backend_hyper::Error;

#[derive(Debug, thiserror::Error)]
pub enum IpfsError {
    #[error("url parse error")]
    Url(#[from] url::ParseError),
    #[error("Failed to send request")]
    Reqwest(#[from] reqwest::Error),
    #[error("http error")]
    Http(#[from] http::Error),
    #[error("Failed to parse scheme")]
    Scheme(#[from] http::uri::InvalidUri),
    #[error("Failed to build client")]
    Client(#[from] IpfsClientError),
    #[error("Failed to parse port")]
    Port(#[from] std::num::ParseIntError),
}
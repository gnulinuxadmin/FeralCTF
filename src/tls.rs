use std::{
    fs::File,
    io::{self, BufReader},
    net::SocketAddr,
    path::Path,
    sync::Arc,
    time::Duration,
};

use axum::serve::Listener;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        crypto::ring,
        pki_types::{CertificateDer, PrivateKeyDer},
    },
    server::TlsStream,
};

use crate::config::ServerConfig as FeralServerConfig;

pub struct TlsListener {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl TlsListener {
    pub async fn bind(addr: SocketAddr, config: &FeralServerConfig) -> Result<Self, anyhow::Error> {
        let certs = load_certificates(&config.tls_cert_path, &config.tls_chain_path)?;
        let key = load_private_key(&config.tls_key_path)?;
        let tls_config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Self {
            listener: TcpListener::bind(addr).await?,
            acceptor: TlsAcceptor::from(Arc::new(tls_config)),
        })
    }
}

impl Listener for TlsListener {
    type Io = TlsStream<TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.listener.accept().await {
                Ok((stream, addr)) => match self.acceptor.accept(stream).await {
                    Ok(tls_stream) => return (tls_stream, addr),
                    Err(err) => {
                        tracing::warn!(%addr, error = %err, "TLS handshake failed");
                    }
                },
                Err(err) => handle_accept_error(err).await,
            }
        }
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

fn load_certificates(
    cert_path: &str,
    chain_path: &str,
) -> Result<Vec<CertificateDer<'static>>, anyhow::Error> {
    let mut certs = read_certificates(cert_path)?;
    let chain_path = chain_path.trim();
    if !chain_path.is_empty() {
        certs.extend(read_certificates(chain_path)?);
    }
    if certs.is_empty() {
        anyhow::bail!("TLS certificate PEM did not contain any certificates");
    }
    Ok(certs)
}

fn read_certificates(
    path: impl AsRef<Path>,
) -> Result<Vec<CertificateDer<'static>>, anyhow::Error> {
    let path = path.as_ref();
    let mut reader = BufReader::new(File::open(path)?);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("{} did not contain any certificates", path.display());
    }
    Ok(certs)
}

fn load_private_key(path: impl AsRef<Path>) -> Result<PrivateKeyDer<'static>, anyhow::Error> {
    let path = path.as_ref();
    let mut reader = BufReader::new(File::open(path)?);
    rustls_pemfile::private_key(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("{} did not contain a private key", path.display()))
}

async fn handle_accept_error(err: io::Error) {
    tracing::warn!(error = %err, "TCP accept failed");
    tokio::time::sleep(Duration::from_secs(1)).await;
}

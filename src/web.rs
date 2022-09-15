use std::sync::Arc;
use actix_cors::Cors;
use actix_web::{App, HttpServer};
use actix_web::middleware::{DefaultHeaders, Logger};
use actix_web::web::Data;
use tokio::sync::mpsc::{channel, Sender};
use tux_lockfree::queue::Queue;
use crate::Config;
use crate::project_processor::{ProjectRequest};
use crate::repository::Repository;

macro_rules! start {
    ($server:tt,$config:tt) => {
    #[cfg(feature = "ssl")]
    {
        if let Some(private) = $config.ssl_private_key {
            let cert = $config
                .ssl_cert_key
                .expect("If Private Key is set. CERT Should be set");
            use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

            let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
            builder
                .set_private_key_file(private, SslFiletype::PEM)
                .unwrap();
            builder.set_certificate_chain_file(cert).unwrap();
             $server.bind_openssl($config.bind_address, builder)?.run().await?;
        }
    }

        $server.bind($config.bind_address)?.run().await?;
    }
}
pub(crate) async fn start(config: Config) -> std::io::Result<()> {
    let (sender, receiver) = channel(100);
    let queue = Data::new(sender);
    tokio::spawn(crate::project_processor::processor(config.cache.clone(), receiver));
    if config.single_repo {
        start_single_server(config, queue).await
    } else {
        start_multi_server(config, queue).await
    }
}

async fn start_single_server(config: Config, queue: Data<Sender<ProjectRequest>>) -> std::io::Result<()> {
    let repository = config.repositories.into_iter().next().unwrap();
    let repository = Data::new(Repository::new(repository.0, repository.1, config.cache.clone()));
    let server = HttpServer::new(move || {
        App::new()
            .app_data(repository.clone())
            .app_data(queue.clone())
            .wrap(DefaultHeaders::new().add(("X-Powered-By", "My Javadoc powered by Actix.rs")))
            .wrap(
                Cors::default()
                    .allow_any_header()
                    .allow_any_method()
                    .allow_any_origin()
                    .supports_credentials(),
            )
            .wrap(Logger::default())
    });
    start!(server, config);
    Ok(())
}

async fn start_multi_server(config: Config, queue: Data<Sender<ProjectRequest>>) -> std::io::Result<()> {
    let repositories = config.repositories.into_iter().map(|(name, data)| {
        Arc::new(Repository::new(name, data, &config.cache))
    }).collect::<Vec<_>>();
    let repositories = Data::new(repositories);
    let server = HttpServer::new(move || {
        App::new()
            .app_data(repositories.clone())
            .app_data(queue.clone())
            .wrap(DefaultHeaders::new().add(("X-Powered-By", "My Javadoc powered by Actix.rs")))
            .wrap(
                Cors::default()
                    .allow_any_header()
                    .allow_any_method()
                    .allow_any_origin()
                    .supports_credentials(),
            )
            .wrap(Logger::default())
            .service(crate::multi::get_javadoc)
    });


    start!(server, config);
    Ok(())
}

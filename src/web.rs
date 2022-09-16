use std::sync::Arc;

use actix_cors::Cors;
use actix_web::middleware::{DefaultHeaders, Logger};
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use handlebars::Handlebars;
use tokio::sync::mpsc::{channel, Sender};

use crate::project_processor::ProjectRequest;
use crate::repository::Repository;
use crate::{Config, site, Templates};

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
                $server
                    .bind_openssl($config.bind_address, builder)?
                    .run()
                    .await?;
            }
        }

        $server.bind($config.bind_address)?.run().await?;
    };
}
pub(crate) async fn start(config: Config) -> std::io::Result<()> {
    let (sender, receiver) = channel(100);
    let queue = Data::new(sender);
    tokio::spawn(crate::project_processor::processor(
        config.cache.clone(),
        receiver,
    ));

    let mut reg = Handlebars::new();
    reg.register_embed_templates::<Templates>().unwrap();
    if config.single_repo {
        start_single_server(config, queue).await
    } else {
        start_multi_server(config, queue, reg).await
    }
}

async fn start_single_server(
    config: Config,
    queue: Data<Sender<ProjectRequest>>,
) -> std::io::Result<()> {
    let repository = config.repositories.into_iter().next().unwrap();
    let repository = Data::new(Repository::new(
        repository.0,
        repository.1,
        config.cache.clone(),
    ));
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

async fn start_multi_server(
    config: Config,
    queue: Data<Sender<ProjectRequest>>,
    reg: Handlebars<'static>,
) -> std::io::Result<()> {
    let repositories = config
        .repositories
        .into_iter()
        .map(|(name, data)| Arc::new(Repository::new(name, data, &config.cache)))
        .collect::<Vec<_>>();
    let repositories = Data::new(repositories);
    let handlebars = Data::new(reg);
    let server = HttpServer::new(move || {
        App::new()
            .app_data(repositories.clone())
            .app_data(queue.clone())
            .app_data(handlebars.clone())
            .wrap(DefaultHeaders::new().add(("X-Powered-By", "My Javadoc powered by Actix.rs")))
            .wrap(
                Cors::default()
                    .allow_any_header()
                    .allow_any_method()
                    .allow_any_origin()
                    .supports_credentials(),
            )
            .wrap(Logger::default())
            .configure(crate::multi::register_web)
            .service(site::index)
    });

    start!(server, config);
    Ok(())
}

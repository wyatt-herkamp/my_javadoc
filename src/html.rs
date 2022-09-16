use std::io::Write;
use lol_html::errors::RewritingError;
use lol_html::html_content::{ContentType, Element};
use lol_html::{element, HtmlRewriter, Settings};
use crate::Resources;

pub fn rewrite_html(
    html: &[u8],
) -> Result<Vec<u8>, RewritingError> {
    let header = Resources::get("header/header.html").unwrap().data;
    let css = Resources::get("header/header.css").unwrap().data;

    let body_handler = |body: &mut Element| {
        body.prepend(String::from_utf8_lossy(header.as_ref()).as_ref(), ContentType::Html);
        Ok(())
    };

    let settings = Settings {
        element_content_handlers: vec![
            element!("body", body_handler),
        ],
        ..Settings::default()
    };

    let mut buffer = Vec::new();
    let mut writer = HtmlRewriter::new(settings, |bytes: &[u8]| {
        buffer.extend_from_slice(bytes);
    });

    writer.write(html)?;
    writer.write(b"<style>")?;
    writer.write(css.as_ref())?;
    writer.write(b"</style>")?;
    writer.end()?;


    Ok(buffer)
}
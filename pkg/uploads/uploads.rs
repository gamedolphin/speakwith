use anyhow::Result;
use futures::Stream;
use tokio::{fs::File, io::BufWriter};
use tokio_util::{bytes::Buf, io::StreamReader};

pub async fn upload_file<B, E>(
    body: impl Stream<Item = std::result::Result<B, E>>,
    base_path: &str,
    id: Option<String>,
    file_name: &str,
) -> Result<String>
where
    B: Buf,
    E: Into<std::io::Error>,
{
    let id = id.unwrap_or_else(|| xid::new().to_string());

    let file_path = format!("{}-{}", id, file_name);
    let path = std::path::Path::new(base_path).join(&file_path);

    let mut file = BufWriter::new(File::create(&path).await?);

    let body_reader = StreamReader::new(body);

    futures::pin_mut!(body_reader);

    tokio::io::copy(&mut body_reader, &mut file).await?;

    Ok(file_path)
}

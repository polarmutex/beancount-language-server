use std::path::PathBuf;
use std::str::FromStr;

pub trait ToFilePath {
    fn to_file_path(&self) -> Result<PathBuf, ()>;
}

impl ToFilePath for lsp_types::Uri {
    fn to_file_path(&self) -> Result<PathBuf, ()> {
        let url = url::Url::from_str(self.as_str()).map_err(|_| ())?;
        tracing::debug!("TOFILEPATH {:#?}", url);
        url.to_file_path()
    }
}

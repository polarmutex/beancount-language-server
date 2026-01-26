use std::path::PathBuf;
use std::str::FromStr;

pub fn file_path_to_uri(path: &std::path::Path) -> Result<lsp_types::Uri, ()> {
    let url = url::Url::from_file_path(path).map_err(|_| ())?;
    lsp_types::Uri::from_str(url.as_str()).map_err(|_| ())
}

pub trait ToFilePath {
    fn to_file_path(&self) -> Result<PathBuf, ()>;
}

impl ToFilePath for lsp_types::Uri {
    fn to_file_path(&self) -> Result<PathBuf, ()> {
        let url = url::Url::from_str(self.as_str()).map_err(|_| ())?;
        url.to_file_path()
    }
}

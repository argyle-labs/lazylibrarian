use plugin_toolkit::prelude::*;

use crate::api::Ll;
use crate::endpoint_db;

pub async fn connect(name: &str) -> Result<Ll> {
    let api_key = plugin_toolkit::secrets::resolve_scoped("lazylibrarian", name, "api_key")?;
    let row = endpoint_db::require(name)?;
    let base_url = route::resolve_reachable(name, &row.routes, false).await?;
    Ok(Ll::new(&base_url, api_key.expose_secret()))
}

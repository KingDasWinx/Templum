//! Templum's HTTP API (docs/specs/api.md): tokens, TLS, the server, the registry and the event
//! stream. No UI: `crates/templum` holds the glue.

mod audit;
pub mod client;
pub mod code;
mod events;
pub mod model;
pub mod openapi;
pub mod registry;
pub mod server;
pub mod store;
mod terminals;
pub mod tls;

pub use model::{Caller, Notice, State};
pub use server::{Client, Options, Server, start};
pub use store::{Scope, Store, Token};
pub use terminals::TerminalRef;

/// This computer's name, as `GET /v1/me` gives it.
pub fn computer() -> String {
    sysinfo::System::host_name().unwrap_or_default()
}

/// The asker of the API's tasks: a terminal id is `<pid>-<n>`, never `api`.
pub const ASKER: &str = "api";
/// A bus `Hello.client` the API's messages carry. The bus refuses it on its socket.
pub const CLIENT: &str = "api";
/// The method of the message that asks the UI thread for the state.
pub const STATE: &str = "templum/state";
/// The method of the notification that says a stream opened: the UI thread starts its tick.
pub const WATCH: &str = "templum/watch";
/// The method of the request a terminal stream's opening sends the UI thread, params
/// `{"block": …}`: the glue checks the block for the token, lends every terminal and answers as a
/// tool does (ok, `isError` with its sentence, or an error code).
pub const TERMINAL: &str = "templum/terminal";
/// The method of the notification that says a client connected or left: the pill counts them.
pub const CLIENTS: &str = "templum/clients";

/// The header a mirror's every request carries: a random value new at each connect or reconnect.
/// A share's token opening an event stream with a new one ends the streams and connections of the
/// old (docs/specs/templum-to-templum.md, The client).
pub const MIRROR_HEADER: &str = "templum-mirror";
/// The name of the event a stream gets (its data is `{}`) just before it ends because another
/// computer connected with the same share.
pub const REPLACED: &str = "replaced";

/// JSON-RPC errors the glue answers, mapped to 403, 401 and 404.
pub mod codes {
    pub const SCOPE: i32 = -32001;
    pub const AUTH: i32 = -32003;
    pub const NOT_FOUND: i32 = -32004;
}

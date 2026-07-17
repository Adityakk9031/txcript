//! Read-only MCP tools over local coding-agent sessions.

use std::path::Path;
use std::process::ExitCode;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::schemars::JsonSchema;
use rmcp::{
    ErrorData, ServerHandler, ServiceExt, tool, tool_handler, tool_router, transport::stdio,
};
use serde::{Deserialize, Serialize};
use txcript::common::Meta;
use txcript::search::{DocMatch, Hit, Origin, Query};
use txcript::{HarnessId, local, text};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListSessionsRequest {
    /// Only include this harness. Omit to include every harness.
    from: Option<String>,
    /// Only include sessions recorded in this working directory. Omit to
    /// include every directory. Sessions without a recorded cwd are excluded
    /// when this filter is present.
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchSessionsRequest {
    /// fzf-style pattern: fuzzy terms, 'exact, ^prefix, suffix$, and !not.
    pattern: String,
    /// Search only this harness. Omit to search every harness.
    from: Option<String>,
    /// Search only sessions recorded in this working directory. Omit to
    /// search every directory. Sessions without a recorded cwd are excluded
    /// when this filter is present.
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadSessionRequest {
    /// Exact session id or exact title.
    id: String,
    /// Only look in this harness. Omit to look across every harness.
    from: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct SessionList {
    sessions: Vec<SessionSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct SessionSummary {
    harness: String,
    id: String,
    timestamp: String,
    title: Option<String>,
    cwd: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
}

impl SessionSummary {
    fn new(harness: HarnessId, meta: &Meta) -> Self {
        Self {
            harness: harness.to_string(),
            id: meta.id.clone(),
            timestamp: meta.timestamp.to_rfc3339(),
            title: meta.title.clone(),
            cwd: meta.cwd.clone(),
            git_branch: meta.git_branch.clone(),
            model: meta.model.clone(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct SearchResults {
    matches: Vec<SearchMatch>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct SearchMatch {
    session: SessionSummary,
    score: u32,
    hits: Vec<SearchHit>,
}

impl SearchMatch {
    fn new(found: &DocMatch<'_>) -> Self {
        Self {
            session: SessionSummary::new(found.key.harness, found.meta),
            score: found.score,
            hits: found.hits.iter().map(SearchHit::from).collect(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
struct SearchHit {
    message: usize,
    block: usize,
    origin: &'static str,
    line: String,
    score: u32,
}

impl From<&Hit> for SearchHit {
    fn from(hit: &Hit) -> Self {
        Self {
            message: hit.message,
            block: hit.block,
            origin: origin_name(hit.origin),
            line: hit.line.clone(),
            score: hit.score,
        }
    }
}

fn origin_name(origin: Origin) -> &'static str {
    match origin {
        Origin::User => "user",
        Origin::Assistant => "assistant",
        Origin::Thinking => "thinking",
        Origin::ToolUse => "tool_use",
        Origin::ToolResult => "tool_result",
        Origin::Meta => "meta",
    }
}

#[derive(Clone)]
struct TxcriptServer {
    tool_router: ToolRouter<Self>,
}

impl TxcriptServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
#[allow(
    clippy::unused_self,
    reason = "rmcp routes tools through methods on the server instance"
)]
impl TxcriptServer {
    /// List local sessions newest-first, with the same harness and working
    /// directory filters as `txcript list`.
    #[tool(
        description = "List local coding-agent sessions newest-first. Optional `from` and `cwd` filters match the txcript CLI; omitted filters include all harnesses or directories.",
        annotations(title = "List sessions", read_only_hint = true)
    )]
    fn list_sessions(
        &self,
        Parameters(request): Parameters<ListSessionsRequest>,
    ) -> Result<Json<SessionList>, ErrorData> {
        let from = parse_from(request.from.as_deref())?;
        let cwd = request.cwd.as_deref().map(Path::new);
        let sessions = local::discover()
            .into_iter()
            .filter(|session| super::selected(session, from, cwd))
            .map(|session| SessionSummary::new(session.harness, &session.meta))
            .collect();
        Ok(Json(SessionList { sessions }))
    }

    /// Search local session content using the same fzf-style pattern, harness,
    /// and working-directory behavior as `txcript query <pattern>`.
    #[tool(
        description = "Search local coding-agent sessions with an fzf-style pattern. Optional `from` and `cwd` filters match the txcript CLI; omitted filters search all harnesses or directories.",
        annotations(title = "Search sessions", read_only_hint = true)
    )]
    fn search_sessions(
        &self,
        Parameters(request): Parameters<SearchSessionsRequest>,
    ) -> Result<Json<SearchResults>, ErrorData> {
        let from = parse_from(request.from.as_deref())?;
        let cwd = request.cwd.as_deref().map(Path::new);
        let index = super::query::index_for(from, cwd);
        let mut query = Query::fuzzy(request.pattern);
        // Match the CLI's one-shot output bounds.
        query.limit = Some(20);
        query.hits_per_doc = Some(3);
        let matches = index.query(&query).iter().map(SearchMatch::new).collect();
        Ok(Json(SearchResults { matches }))
    }

    /// Read one session by exact id or title and return its token-optimized
    /// text projection. The optional harness scope behaves like `--from` on
    /// `txcript continue`.
    #[tool(
        description = "Read a local session by exact id or title as token-optimized text. Omit `from` to search every harness.",
        annotations(title = "Read session", read_only_hint = true)
    )]
    fn read_session(
        &self,
        Parameters(request): Parameters<ReadSessionRequest>,
    ) -> Result<String, ErrorData> {
        let from = parse_from(request.from.as_deref())?;
        let sessions = local::discover();
        let found = sessions.iter().find(|session| {
            from.is_none_or(|harness| session.harness == harness)
                && (session.meta.id == request.id
                    || session.meta.title.as_deref() == Some(request.id.as_str()))
        });
        let session = found.ok_or_else(|| {
            let scope = from.map_or(String::new(), |harness| format!(" {harness}"));
            ErrorData::invalid_params(
                format!("no local{scope} session matches `{}`", request.id),
                None,
            )
        })?;
        let common = session.read().map_err(|error| {
            ErrorData::internal_error(format!("reading session `{}`: {error}", request.id), None)
        })?;
        Ok(text::to_text(&common))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TxcriptServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("txcript", env!("CARGO_PKG_VERSION"))
                    .with_title("txcript session server")
                    .with_description("Find, search, and read local coding-agent sessions"),
            )
            .with_instructions(
                "Use list_sessions to browse, search_sessions to find content, and read_session to retrieve token-optimized context. Omitted `from` and `cwd` filters include all harnesses and directories.",
            )
    }
}

fn parse_from(from: Option<&str>) -> Result<Option<HarnessId>, ErrorData> {
    from.map(str::parse).transpose().map_err(|error| {
        ErrorData::invalid_params(
            format!("{error}; expected one of: {}", super::HARNESSES),
            None,
        )
    })
}

pub async fn serve() -> Result<ExitCode, String> {
    let service = TxcriptServer::new()
        .serve(stdio())
        .await
        .map_err(|error| format!("starting MCP stdio server: {error}"))?;
    service
        .waiting()
        .await
        .map_err(|error| format!("running MCP stdio server: {error}"))?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_exactly_the_three_session_tools() {
        let mut names = TxcriptServer::new()
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["list_sessions", "read_session", "search_sessions"]);
    }

    #[test]
    fn list_and_search_schemas_expose_cli_filters_as_optional() {
        let list = TxcriptServer::list_sessions_tool_attr();
        assert!(list.input_schema["properties"].get("from").is_some());
        assert!(list.input_schema["properties"].get("cwd").is_some());
        assert!(list.input_schema.get("required").is_none());

        let search = TxcriptServer::search_sessions_tool_attr();
        assert!(search.input_schema["properties"].get("pattern").is_some());
        assert!(search.input_schema["properties"].get("from").is_some());
        assert!(search.input_schema["properties"].get("cwd").is_some());
        assert_eq!(
            search.input_schema["required"],
            serde_json::json!(["pattern"])
        );
    }

    #[test]
    fn omitted_from_means_every_harness_and_aliases_still_work() {
        assert_eq!(parse_from(None).ok(), Some(None));
        assert_eq!(
            parse_from(Some("claude")).ok(),
            Some(Some(HarnessId::ClaudeCode))
        );
        assert!(parse_from(Some("not-a-harness")).is_err());
    }
}

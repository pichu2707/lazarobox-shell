//! The entries, in the order of C8. Binary names marked U in the design stay
//! unverified until a manual install pins them.

use super::{Class, Entry, Group, Link, Login, TUTORIAL, Var, Way};

pub(super) static ENTRIES: &[Entry] = &[CLARITY, GA4, GSC, GOOGLE, APPS_SCRIPT];

/// `GSC_SKIP_OAUTH = "true"`: no browser flow, ever (the server would start one
/// inside an MCP call).
const SKIP: Var = Var {
    name: "GSC_SKIP_OAUTH",
    shown: true,
    preset: Some("true"),
    ..Var::SECRET
};

/// The OAuth way of `gsc`: the client file of the GA4 guide, a fixed token
/// folder (mcp-gsc does not expand `~`, and a chat gets a scrubbed `XDG_*`),
/// and the browser flow on.
const OAUTH_FILE: Var = Var {
    name: "GSC_OAUTH_CLIENT_SECRETS_FILE",
    shown: true,
    preset: Some(".config/lazarobox/google-oauth-client.json"),
    under_home: true,
    ..Var::SECRET
};
const OAUTH_DIR: Var = Var {
    name: "GSC_CONFIG_DIR",
    shown: true,
    preset: Some(".config/mcp-gsc"),
    under_home: true,
    ..Var::SECRET
};
const OAUTH_ON: Var = Var {
    preset: Some("false"),
    ..SKIP
};
const KEY_PATH: Var = Var {
    name: "GSC_CREDENTIALS_PATH",
    shown: true,
    ..Var::SECRET
};

const CLARITY: Entry = Entry {
    id: "clarity",
    title: "Clarity",
    group: Group::Analytics,
    project: "microsoft/clarity-mcp-server",
    license: "MIT",
    install: "npm i -g @microsoft/clarity-mcp-server",
    command: "clarity-mcp-server",
    args: &[],
    env: &[Var {
        name: "CLARITY_API_TOKEN",
        ..Var::SECRET
    }],
    ways: &[],
    login: None,
    link: Link::None,
    hidden: &[],
    gmail_allowlist: false,
    guide: &[
        "1. In the Clarity project settings, open Data Export.",
        "2. Generate a new API token with project admin rights and copy it.",
        "3. Put it in secrets.toml under [clarity] as CLARITY_API_TOKEN.",
        "Limits of the Clarity API: 10 requests per project per day, the last 1 to 3 days only, at most 1,000 rows per answer.",
        "Guía en español: docs/conectores.md (último enlace).",
    ],
    links: &["https://clarity.microsoft.com/", TUTORIAL],
    token_location: "secrets.toml, [clarity] CLARITY_API_TOKEN",
    tools: &[
        ("query-analytics-dashboard", Class::Read),
        ("list-session-recordings", Class::Read),
        ("query-documentation-resources", Class::Read),
    ],
};

const GA4: Entry = Entry {
    id: "ga4",
    title: "GA4",
    group: Group::Analytics,
    project: "googleanalytics/google-analytics-mcp",
    license: "Apache-2.0",
    install: "pipx install analytics-mcp",
    command: "analytics-mcp",
    args: &[],
    env: &[Var {
        name: "GOOGLE_CLOUD_PROJECT",
        shown: true,
        ..Var::SECRET
    }],
    ways: &[],
    link: Link::Login,
    hidden: &[],
    gmail_allowlist: false,
    login: Some(Login {
        line: "gcloud auth application-default login --scopes https://www.googleapis.com/auth/analytics.readonly,https://www.googleapis.com/auth/cloud-platform --client-id-file=~/.config/lazarobox/google-oauth-client.json",
    }),
    guide: &[
        "1. In your own Google Cloud project, enable the Google Analytics Admin API and the Google Analytics Data API.",
        "2. Under APIs and services, Credentials, create an OAuth client of type Desktop app and download its JSON file.",
        "3. Save the file as ~/.config/lazarobox/google-oauth-client.json (lazarobox refuses to read it).",
        "4. Put your project id in secrets.toml under [ga4] as GOOGLE_CLOUD_PROJECT, then use Log in.",
        "Warning: a client left in \"Testing\" gets refresh tokens that expire after 7 days. Publish the consent screen to avoid logging in every week.",
        "Guía en español: docs/conectores.md (último enlace).",
    ],
    links: &[
        "https://console.cloud.google.com/apis/credentials",
        "https://github.com/googleanalytics/google-analytics-mcp",
        TUTORIAL,
    ],
    token_location: "~/.config/gcloud (application default credentials)",
    tools: &[
        ("get_account_summaries", Class::Read),
        ("get_property_details", Class::Read),
        ("list_google_ads_links", Class::Read),
        ("list_property_annotations", Class::Read),
        ("get_custom_dimensions_and_metrics", Class::Read),
        ("run_report", Class::Read),
        ("run_funnel_report", Class::Read),
        ("run_realtime_report", Class::Read),
        ("list_key_events", Class::Read),
        ("run_conversions_report", Class::Read),
    ],
};

const GSC: Entry = Entry {
    id: "gsc",
    title: "Search Console",
    group: Group::Analytics,
    project: "AminForou/mcp-gsc",
    license: "MIT",
    install: "uv tool install mcp-search-console==0.4.2",
    command: "mcp-search-console",
    args: &[],
    env: &[OAUTH_FILE, OAUTH_DIR, KEY_PATH, SKIP],
    ways: &[
        Way {
            title: "OAuth",
            vars: &[OAUTH_FILE, OAUTH_DIR, OAUTH_ON],
            login: None,
        },
        Way {
            title: "Service account",
            vars: &[KEY_PATH, SKIP],
            login: None,
        },
    ],
    login: None,
    link: Link::Login,
    hidden: &["reauthenticate"],
    gmail_allowlist: false,
    guide: &[
        "Two ways. Declare the server with name = \"gsc\": the hidden tool and the start check apply to that name only.",
        "OAuth (your own client, the default):",
        "1. In your own Google Cloud project, enable the Google Search Console API and create an OAuth client of type Desktop app (the one of the GA4 guide works).",
        "2. Add the OAuth client in Connectors (o) or save its JSON as ~/.config/lazarobox/google-oauth-client.json. If the config folder is a symlink the menu save refuses; hand-editing still works.",
        "3. Fill the values (f), keeping GSC_CONFIG_DIR under ~/.config/mcp-gsc (a start from another folder is refused), then use Link with Google (g): the browser opens once. The Link command creates the token and its folder private (umask 077); for a token made another way, run chmod 700 ~/.config/mcp-gsc and chmod 600 ~/.config/mcp-gsc/token.json. Link before the first chat call: a call made before linking is refused at start, and if calls hang, link again (Esc cancels the call).",
        "Service account:",
        "1. In your own Google Cloud project, enable the Google Search Console API and create a service account.",
        "2. Create a JSON key for it and save it as ~/.config/lazarobox/gsc-service-account.json with mode 0600 (lazarobox refuses to read it).",
        "3. In Search Console, open Settings, Users and permissions, and add the service account e-mail as a user of the property (Restricted is enough for search analytics; if URL inspection is denied, use Full).",
        "4. Choose this way (w), then put the path of the JSON file in secrets.toml under [gsc] as GSC_CREDENTIALS_PATH and set GSC_SKIP_OAUTH to \"true\". There is no login step.",
        "The reauthenticate tool is hidden from chats: it deletes the token and starts a browser OAuth flow.",
        "Guía en español: docs/conectores.md (último enlace).",
    ],
    links: &[
        "https://console.cloud.google.com/apis/credentials",
        "https://console.cloud.google.com/iam-admin/serviceaccounts",
        "https://search.google.com/search-console/users",
        "https://github.com/AminForou/mcp-gsc",
        TUTORIAL,
    ],
    token_location: "~/.config/mcp-gsc/token.json (OAuth) or the service-account JSON key (GSC_CREDENTIALS_PATH)",
    tools: &[
        ("get_capabilities", Class::Read),
        ("get_creator_info", Class::Read),
        ("list_properties", Class::Read),
        ("get_site_details", Class::Read),
        ("get_search_analytics", Class::Read),
        ("get_performance_overview", Class::Read),
        ("get_advanced_search_analytics", Class::Read),
        ("compare_search_periods", Class::Read),
        ("get_search_by_page_query", Class::Read),
        ("inspect_url_enhanced", Class::Read),
        ("batch_url_inspection", Class::Read),
        ("check_indexing_issues", Class::Read),
        ("get_sitemaps", Class::Read),
        ("list_sitemaps_enhanced", Class::Read),
        ("get_sitemap_details", Class::Read),
        ("manage_sitemaps", Class::Write),
        ("submit_sitemap", Class::Write),
        ("delete_sitemap", Class::Write),
        ("add_site", Class::Write),
        ("delete_site", Class::Write),
        ("reauthenticate", Class::Write),
    ],
};

const GOOGLE: Entry = Entry {
    id: "google",
    title: "Google (Gmail, Docs, Sheets)",
    group: Group::Workspace,
    project: "taylorwilsdon/google_workspace_mcp",
    license: "MIT",
    install: "uv tool install workspace-mcp==2.4.1",
    command: "workspace-mcp",
    args: &[
        "--single-user",
        "--permissions",
        "gmail:readonly+drafts",
        "docs:full",
        "sheets:full",
        "--disabled-tools",
        "send_gmail_message",
    ],
    env: &[
        Var {
            name: "GOOGLE_OAUTH_CLIENT_ID",
            shown: true,
            ..Var::SECRET
        },
        Var {
            name: "GOOGLE_OAUTH_CLIENT_SECRET",
            ..Var::SECRET
        },
        Var {
            name: "USER_GOOGLE_EMAIL",
            shown: true,
            ..Var::SECRET
        },
        Var {
            name: "WORKSPACE_MCP_DISABLE_LOCAL_FILES",
            shown: true,
            preset: Some("true"),
            ..Var::SECRET
        },
    ],
    ways: &[],
    login: None,
    link: Link::GoogleAuth,
    hidden: &["start_google_auth"],
    gmail_allowlist: true,
    guide: &[
        "Declare the server with name = \"google\": the Gmail allowlist and the hidden tools are keyed by that name, and under another name every Gmail tool is registered.",
        "1. In your own Google Cloud project, enable the Gmail, Docs, Sheets and Drive APIs and create an OAuth client of type Desktop app.",
        "2. Put its id, its secret and your Google e-mail in secrets.toml under [google].",
        "3. Use Link with Google: the browser opens once and the server keeps the token.",
        "Warning: a client left in \"Testing\" gets refresh tokens that expire after 7 days. Publish the consent screen to avoid linking every week.",
        "Replies are created as Gmail drafts; you send them from Gmail. lazarobox never registers a send tool.",
        "The server holds a gmail.compose token that could send at the API level: this is the accepted residual risk.",
        "workspace-mcp keeps its tokens as plaintext JSON in ~/.google_workspace_mcp/credentials (lazarobox refuses to read that folder).",
        "The Docs and Sheets permissions include drive.readonly, so the server can read your whole Drive.",
        "get_gmail_attachment_content saves attachments to local disk (~/.workspace-mcp/attachments) outside lazarobox's write checks: every call asks y.",
        "WORKSPACE_MCP_DISABLE_LOCAL_FILES is \"true\": draft_gmail_message cannot attach local files by path (checked on 2.4.1; the other tools keep working). Set it to \"false\" to allow it.",
        "Guía en español: docs/conectores.md (último enlace).",
    ],
    links: &[
        "https://console.cloud.google.com/apis/credentials",
        "https://github.com/taylorwilsdon/google_workspace_mcp",
        TUTORIAL,
    ],
    token_location: "~/.google_workspace_mcp/credentials (plaintext JSON)",
    tools: &[
        ("start_google_auth", Class::Write),
        ("search_gmail_messages", Class::Read),
        ("get_gmail_message_content", Class::Read),
        ("get_gmail_messages_content_batch", Class::Read),
        ("get_gmail_thread_content", Class::Read),
        ("get_gmail_threads_content_batch", Class::Read),
        ("list_gmail_labels", Class::Read),
        ("get_gmail_attachment_content", Class::Write),
        ("draft_gmail_message", Class::Write),
        ("search_docs", Class::Read),
        ("get_doc_content", Class::Read),
        ("list_docs_in_folder", Class::Read),
        ("inspect_doc_structure", Class::Read),
        ("debug_table_structure", Class::Read),
        ("get_doc_as_markdown", Class::Read),
        ("list_document_comments", Class::Read),
        ("create_doc", Class::Write),
        ("modify_doc_text", Class::Write),
        ("find_and_replace_doc", Class::Write),
        ("insert_doc_elements", Class::Write),
        ("insert_doc_image", Class::Write),
        ("update_doc_headers_footers", Class::Write),
        ("batch_update_doc", Class::Write),
        ("create_table_with_data", Class::Write),
        ("export_doc_to_pdf", Class::Write),
        ("update_paragraph_style", Class::Write),
        ("manage_doc_tab", Class::Write),
        ("list_spreadsheets", Class::Read),
        ("get_spreadsheet_info", Class::Read),
        ("read_sheet_values", Class::Read),
        ("list_sheet_tables", Class::Read),
        ("list_spreadsheet_comments", Class::Read),
        ("modify_sheet_values", Class::Write),
        ("format_sheet_range", Class::Write),
        ("manage_conditional_formatting", Class::Write),
        ("create_spreadsheet", Class::Write),
        ("create_sheet", Class::Write),
        ("append_table_rows", Class::Write),
        ("manage_sheet_tab", Class::Write),
        ("resize_sheet_dimensions", Class::Write),
        ("move_sheet_rows", Class::Write),
        ("manage_named_range", Class::Write),
    ],
};

const APPS_SCRIPT: Entry = Entry {
    id: "apps-script",
    title: "Apps Script",
    group: Group::Workspace,
    project: "google/clasp",
    license: "Apache-2.0",
    install: "npm i -g @google/clasp@3.4.1",
    command: "clasp",
    args: &["mcp"],
    env: &[],
    ways: &[],
    login: Some(Login {
        line: "clasp login",
    }),
    link: Link::Login,
    hidden: &[],
    gmail_allowlist: false,
    guide: &[
        "1. Switch on the Apps Script API for your account at https://script.google.com/home/usersettings.",
        "2. Use Log in: clasp opens the browser with its own OAuth client and keeps the token in ~/.clasprc.json (lazarobox refuses to read it).",
        "Version 3.4.0 or later is required (it fixed the project-directory path validation).",
        "push_files uploads code from a directory on disk, and pushed code runs with the script's scopes.",
        "pull_files and clone_project write local files and may overwrite them.",
        "Guía en español: docs/conectores.md (último enlace).",
    ],
    links: &[
        "https://script.google.com/home/usersettings",
        "https://github.com/google/clasp",
        TUTORIAL,
    ],
    token_location: "~/.clasprc.json",
    tools: &[
        ("list_projects", Class::Read),
        ("push_files", Class::Write),
        ("create_project", Class::Write),
        ("pull_files", Class::Write),
        ("clone_project", Class::Write),
    ],
};

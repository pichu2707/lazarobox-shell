//! The entries, in the order of C8. Binary names marked U in the design stay
//! unverified until a manual install pins them.

use super::{Class, Entry, Group, Login, Var};

pub(super) static ENTRIES: &[Entry] = &[CLARITY, GA4];

const CLARITY: Entry = Entry {
    id: "clarity",
    group: Group::Analytics,
    project: "microsoft/clarity-mcp-server",
    license: "MIT",
    install: "npm i -g @microsoft/clarity-mcp-server",
    command: "clarity-mcp-server",
    args: &[],
    env: &[Var {
        name: "CLARITY_API_TOKEN",
        secret: true,
        preset: None,
    }],
    login: None,
    guide: &[
        "1. In the Clarity project settings, open Data Export.",
        "2. Generate a new API token with project admin rights and copy it.",
        "3. Put it in secrets.toml under [clarity] as CLARITY_API_TOKEN.",
        "Limits of the Clarity API: 10 requests per project per day, the last 1 to 3 days only, at most 1,000 rows per answer.",
    ],
    links: &["https://clarity.microsoft.com/"],
    token_location: "secrets.toml, [clarity] CLARITY_API_TOKEN",
    tools: &[
        ("query-analytics-dashboard", Class::Read),
        ("list-session-recordings", Class::Read),
        ("query-documentation-resources", Class::Read),
    ],
};

const GA4: Entry = Entry {
    id: "ga4",
    group: Group::Analytics,
    project: "googleanalytics/google-analytics-mcp",
    license: "Apache-2.0",
    install: "pipx install analytics-mcp",
    command: "analytics-mcp",
    args: &[],
    env: &[Var {
        name: "GOOGLE_CLOUD_PROJECT",
        secret: true,
        preset: None,
    }],
    login: Some(Login {
        line: "gcloud auth application-default login --scopes https://www.googleapis.com/auth/analytics.readonly,https://www.googleapis.com/auth/cloud-platform --client-id-file=~/.config/lazarobox/google-oauth-client.json",
    }),
    guide: &[
        "1. In your own Google Cloud project, enable the Google Analytics Admin API and the Google Analytics Data API.",
        "2. Under APIs and services, Credentials, create an OAuth client of type Desktop app and download its JSON file.",
        "3. Save the file as ~/.config/lazarobox/google-oauth-client.json (lazarobox refuses to read it).",
        "4. Put your project id in secrets.toml under [ga4] as GOOGLE_CLOUD_PROJECT, then use Log in.",
        "Warning: a client left in \"Testing\" gets refresh tokens that expire after 7 days. Publish the consent screen to avoid logging in every week.",
    ],
    links: &[
        "https://console.cloud.google.com/apis/credentials",
        "https://github.com/googleanalytics/google-analytics-mcp",
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
        ("create_property_annotation", Class::Write),
        ("create_custom_dimension", Class::Write),
        ("create_key_event", Class::Write),
    ],
};

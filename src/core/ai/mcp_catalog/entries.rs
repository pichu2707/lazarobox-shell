//! The entries, in the order of C8. Binary names marked U in the design stay
//! unverified until a manual install pins them.

use super::{Class, Entry, Group, Login, Var};

pub(super) static ENTRIES: &[Entry] = &[CLARITY, GA4, GSC];

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
    title: "GA4",
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

const GSC: Entry = Entry {
    id: "gsc",
    title: "Search Console",
    group: Group::Analytics,
    project: "AminForou/mcp-gsc",
    license: "MIT",
    install: "uv tool install mcp-search-console",
    command: "mcp-search-console",
    args: &[],
    env: &[
        Var {
            name: "GSC_CREDENTIALS_PATH",
            secret: true,
            preset: None,
        },
        Var {
            name: "GSC_SKIP_OAUTH",
            secret: true,
            preset: Some("true"),
        },
    ],
    login: None,
    guide: &[
        "1. In your own Google Cloud project, enable the Google Search Console API and create a service account.",
        "2. Create a JSON key for it and save it as ~/.config/lazarobox/gsc-service-account.json with mode 0600 (lazarobox refuses to read it).",
        "3. In Search Console, open Settings, Users and permissions, and add the service account e-mail as a user of the property (Restricted is enough to read).",
        "4. Put the path of the JSON file in secrets.toml under [gsc] as GSC_CREDENTIALS_PATH; GSC_SKIP_OAUTH is already \"true\". There is no login step.",
    ],
    links: &[
        "https://console.cloud.google.com/iam-admin/serviceaccounts",
        "https://search.google.com/search-console/users",
        "https://github.com/AminForou/mcp-gsc",
    ],
    token_location: "the service-account JSON key (GSC_CREDENTIALS_PATH)",
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

use super::*;

#[tokio::test]
async fn tags_and_statistics() {
    tags().await;
    covered!(
        forge_updates,
        "GET /updates/demo/forge_updates.json?neoforge=include",
        Operation::ForgeUpdates(ProjectKey::new("demo").unwrap(), ForgeLoader::Both),
        ForgeUpdates,
        r#"{"homepage":"https://modrinth.com/mod/demo","promos":{"1.21-latest":"2.0"},"1.21":{"2.0":""}}"#
    );
    covered!(
        statistics,
        "GET /v2/statistics",
        Operation::Statistics,
        Statistics,
        r#"{"projects":1,"versions":2,"authors":3,"files":4}"#
    );
    covered!(
        analytics_events,
        "GET /v3/analytics-event",
        Operation::AnalyticsEvents,
        AnalyticsEvents,
        r#"[{"id":"AABBCCDD","title":"Outage","for_metric_kind":["downloads"],"starts":"2026-11-28T00:00:00Z","ends":"2026-11-28T01:00:00Z"}]"#
    );
}

async fn tags() {
    covered!(
        categories,
        "GET /v2/tag/category",
        Operation::Tags(Tags::Categories),
        Categories,
        r#"[{"name":"adventure","icon":"","project_type":"mod","header":"categories"}]"#
    );
    covered!(
        loaders,
        "GET /v2/tag/loader",
        Operation::Tags(Tags::Loaders),
        Loaders,
        r#"[{"name":"fabric","icon":"","supported_project_types":["mod"]}]"#
    );
    let filter = GameVersionFilter {
        version_type: Some(GameVersionType::Release),
        major: Some(true),
    };
    covered!(
        game_versions,
        "GET /v2/tag/game_version?type=release&major=true",
        Operation::GameVersions(filter),
        GameVersions,
        r#"[{"version":"1.21","version_type":"release","date":"2026-11-28T00:00:00Z","major":true}]"#
    );
    covered!(
        licenses,
        "GET /v2/tag/license",
        Operation::Tags(Tags::Licenses),
        Licenses,
        r#"[{"short":"MIT","name":"MIT License"}]"#
    );
    covered!(
        license_text,
        "GET /v2/tag/license/MIT",
        Operation::License(LicenseId::new("MIT").unwrap()),
        LicenseText,
        r#"{"title":"MIT License","body":"Terms"}"#
    );
    provider_tags().await;
}

async fn provider_tags() {
    covered!(
        donation_platforms,
        "GET /v2/tag/donation_platform",
        Operation::Tags(Tags::DonationPlatforms),
        DonationPlatforms,
        r#"[{"short":"patreon","name":"Patreon"}]"#
    );
    covered!(
        link_platforms,
        "GET /v3/link_platform",
        Operation::Tags(Tags::LinkPlatforms),
        LinkPlatforms,
        r#"[{"name":"patreon","donation":true}]"#
    );
    covered!(
        report_types,
        "GET /v2/tag/report_type",
        Operation::Tags(Tags::ReportTypes),
        TagNames,
        r#"["spam"]"#
    );
    covered!(
        project_types,
        "GET /v2/tag/project_type",
        Operation::Tags(Tags::ProjectTypes),
        TagNames,
        r#"["mod"]"#
    );
    covered!(
        side_types,
        "GET /v2/tag/side_type",
        Operation::Tags(Tags::SideTypes),
        TagNames,
        r#"["required"]"#
    );
    covered!(
        loader_values,
        "GET /v3/loader_field?loader_field=environment",
        Operation::LoaderField(LoaderFieldQuery::Environment),
        LoaderValues,
        r#"[{"value":"client_only","created":"1970-01-01T00:00:00Z"}]"#
    );
}

#[tokio::test]
async fn public_payout_metadata() {
    let country = CountryCode::new("us").unwrap();
    covered!(
        payment_methods,
        "GET /v3/payout/methods?country=US",
        Operation::PaymentMethods(Some(country)),
        PaymentMethods,
        r#"[{"id":"paypal_us","type":"paypal","name":"PayPal","interval":{"standard":{"min":0.25,"max":"1000.00"}}},{"id":"demo","type":"tremendous","name":"Gift card","interval":{"fixed":{"values":[10.0,20.0]}}}]"#
    );
    covered!(
        platform_revenue,
        "GET /v3/payout/platform_revenue",
        Operation::PlatformRevenue(RevenueRange::default()),
        PlatformRevenue,
        r#"{"all_time":"12.40","all_time_available":"12.40","data":[{"time":1,"revenue":"2.00","creator_revenue":"6.00"}]}"#
    );
}

#[tokio::test]
async fn maven_descriptors_hashes_and_redirects() {
    let project = ProjectKey::new("demo").unwrap();
    let coordinate = MavenCoordinate {
        project: project.clone(),
        version: VersionKey::new("1.0").unwrap(),
        filename: "demo-1.0.jar".into(),
    };
    covered!(
        maven_metadata,
        "GET /maven/maven/modrinth/demo/maven-metadata.xml",
        Operation::MavenMetadata(project),
        MavenMetadata,
        "<metadata><groupId>maven.modrinth</groupId><artifactId>demo</artifactId><versioning><latest>1.0</latest><release>1.0</release><versions><version>1.0</version></versions><lastUpdated>20261128</lastUpdated></versioning></metadata>"
    );
    covered!(
        maven_artifact,
        "GET /maven/maven/modrinth/demo/1.0/demo-1.0.pom",
        Operation::MavenArtifact(MavenCoordinate {
            filename: "demo-1.0.pom".into(),
            ..coordinate.clone()
        }),
        MavenArtifact,
        "<project><modelVersion>4.0.0</modelVersion><groupId>maven.modrinth</groupId><artifactId>demo</artifactId><version>1.0</version><name>Demo</name><description>Example project</description></project>"
    );
    covered!(
        maven_artifact,
        "GET /maven/maven/modrinth/demo/1.0/demo-1.0.jar",
        Operation::MavenArtifact(coordinate.clone()),
        MavenArtifact,
        redirect
    );
    covered!(
        maven_head,
        "HEAD /maven/maven/modrinth/demo/1.0/demo-1.0.jar",
        Operation::MavenHead(coordinate.clone()),
        MavenHead,
        redirect
    );
    covered!(
        maven_hash,
        "GET /maven/maven/modrinth/demo/1.0/demo-1.0.jar.sha1",
        Operation::MavenHash(coordinate.clone(), HashAlgorithm::Sha1),
        MavenHash,
        HASH
    );
    covered!(
        maven_hash,
        "GET /maven/maven/modrinth/demo/1.0/demo-1.0.jar.sha512",
        Operation::MavenHash(coordinate, HashAlgorithm::Sha512),
        MavenHash,
        "a".repeat(128)
    );
}

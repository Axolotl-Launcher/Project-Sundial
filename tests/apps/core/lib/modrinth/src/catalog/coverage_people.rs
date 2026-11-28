use super::*;

const USER: &str = r#"{"id":"EEFFGGHH","username":"example","name":null,"avatar_url":null,"bio":null,"created":"2026-01-01T00:00:00Z","role":"developer","badges":0,"email":"private@example.invalid"}"#;
const MEMBER: &str = r#"{"team_id":"QQRRSSTT","user":{"id":"EEFFGGHH","username":"example"},"role":"Maintainer","ordering":0,"is_owner":true}"#;
const ORGANIZATION: &str = r#"{"id":"UUVVWWXX","slug":"studio","name":"Studio","team_id":"QQRRSSTT","description":"Public team","members":[]}"#;
const COLLECTION: &str = r#"{"id":"AABBCCTT","user":"EEFFGGHH","name":"Examples","description":null,"status":"listed","created":"2026-11-28T00:00:00Z","updated":"2026-11-28T00:00:00Z","projects":["AABBCCDD"]}"#;

#[tokio::test]
async fn users_and_teams() {
    let user = UserKey::new("example").unwrap();
    let users = Batch::new(vec![UserKey::new("EEFFGGHH").unwrap()]).unwrap();
    let project = ProjectKey::new("demo").unwrap();
    let team = TeamId::new("QQRRSSTT").unwrap();
    covered!(
        get_user,
        "GET /v2/user/example",
        Operation::User(user.clone()),
        User,
        USER
    );
    covered!(
        get_users,
        "GET /v2/users?ids=%5B%22EEFFGGHH%22%5D",
        Operation::Users(users),
        Users,
        format!("[{USER}]")
    );
    covered!(
        search_users,
        "GET /v3/users/search?query=Exam",
        Operation::SearchUsers("Exam".into()),
        SearchUsers,
        r#"[{"id":"EEFFGGHH","username":"example","avatar_url":null}]"#
    );
    covered!(
        user_projects,
        "GET /v2/user/example/projects",
        Operation::UserProjects(user.clone()),
        Projects,
        "[]"
    );
    covered!(
        user_all_projects,
        "GET /v3/user/example/all-projects",
        Operation::UserAllProjects(user),
        AllProjects,
        r#"{"projects":[{"id":"AABBCCDD","slug":"demo","name":"Demo","summary":"A sample","description":"Body","project_types":["mod"],"downloads":5}],"organizations":{}}"#
    );
    covered!(
        project_members,
        "GET /v2/project/demo/members",
        Operation::ProjectMembers(project),
        Members,
        format!("[{MEMBER}]")
    );
    covered!(
        team_members,
        "GET /v2/team/QQRRSSTT/members",
        Operation::TeamMembers(team.clone()),
        Members,
        format!("[{MEMBER}]")
    );
    covered!(
        teams,
        "GET /v2/teams?ids=%5B%22QQRRSSTT%22%5D",
        Operation::Teams(Batch::new(vec![team]).unwrap()),
        Teams,
        format!("[[{MEMBER}]]")
    );
}

#[tokio::test]
async fn organizations_and_collections() {
    let organization = ProjectKey::new("studio").unwrap();
    let user = UserKey::new("example").unwrap();
    covered!(
        get_organization,
        "GET /v3/organization/studio",
        Operation::Organization(organization.clone()),
        Organization,
        ORGANIZATION
    );
    covered!(
        get_organizations,
        "GET /v3/organizations?ids=%5B%22studio%22%5D",
        Operation::Organizations(Batch::new(vec![organization.clone()]).unwrap()),
        Organizations,
        format!("[{ORGANIZATION}]")
    );
    covered!(
        project_organization,
        "GET /v3/project/demo/organization",
        Operation::ProjectOrganization(ProjectKey::new("demo").unwrap()),
        Organization,
        ORGANIZATION
    );
    covered!(
        organization_projects,
        "GET /v3/organization/studio/projects",
        Operation::OrganizationProjects(organization.clone()),
        OrganizationProjects,
        "[]"
    );
    covered!(
        organization_members,
        "GET /v3/organization/studio/members",
        Operation::OrganizationMembers(organization),
        Members,
        format!("[{MEMBER}]")
    );
    covered!(
        user_organizations,
        "GET /v3/user/example/organizations",
        Operation::UserOrganizations(user.clone()),
        Organizations,
        format!("[{ORGANIZATION}]")
    );
    collections(user).await;
}

async fn collections(user: UserKey) {
    let collection = CollectionId::new("AABBCCTT").unwrap();
    covered!(
        get_collection,
        "GET /v3/collection/AABBCCTT",
        Operation::Collection(collection.clone()),
        Collection,
        COLLECTION
    );
    covered!(
        get_collections,
        "GET /v3/collections?ids=%5B%22AABBCCTT%22%5D",
        Operation::Collections(Batch::new(vec![collection]).unwrap()),
        Collections,
        format!("[{COLLECTION}]")
    );
    covered!(
        user_collections,
        "GET /v3/user/example/collections",
        Operation::UserCollections(user),
        Collections,
        "[]"
    );
}

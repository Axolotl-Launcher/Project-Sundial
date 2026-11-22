// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::DesktopOperation;

operation_groups! {
    cache => "cache" {
        CANCEL_SEARCH_REQUEST => "cancel_search_request",
        GET_ORGANIZATION => "get_organization",
        GET_ORGANIZATION_MANY => "get_organization_many",
        GET_PROJECT_VERSIONS => "get_project_versions",
        GET_SEARCH_RESULTS => "get_search_results",
        GET_SEARCH_RESULTS_MANY => "get_search_results_many",
        GET_SEARCH_RESULTS_V3 => "get_search_results_v3",
        GET_SEARCH_RESULTS_V3_MANY => "get_search_results_v3_many",
        GET_TEAM => "get_team",
        GET_TEAM_MANY => "get_team_many",
        GET_USER => "get_user",
        GET_USER_MANY => "get_user_many",
        PURGE_CACHE_TYPES => "purge_cache_types",
    }

    content_search => "content-search" {
        EXPAND_CONTENT_SEARCH_QUERY => "expand_content_search_query",
        LOOKUP_CHINESE_CONTENT_NAMES => "lookup_chinese_content_names",
        LOOKUP_CONTENT_IDENTITIES => "lookup_content_identities",
        LOOKUP_CONTENT_WIKI_IDS => "lookup_content_wiki_ids",
        RESOLVE_CHINESE_CONTENT_SEARCH => "resolve_chinese_content_search",
    }

    curseforge => "curseforge" {
        API_KEY_OVERRIDE => "curseforge_api_key_override",
        CAPABILITY => "curseforge_capability",
        CONFIGURE_MANUAL_DOWNLOAD_WATCHER => "curseforge_configure_manual_download_watcher",
        GET_CATEGORIES => "curseforge_get_categories",
        GET_DOWNLOAD_URL => "curseforge_get_download_url",
        IMPORT_MANUAL_DOWNLOADS => "curseforge_import_manual_downloads",
        IMPORT_PENDING_MANUAL_DOWNLOAD_FILE => "curseforge_import_pending_manual_download_file",
        INSTALL_MODPACK => "curseforge_install_modpack",
        LIST_PENDING_MANUAL_DOWNLOADS => "curseforge_list_pending_manual_downloads",
        RECOGNIZE_INSTANCE_FILES => "curseforge_recognize_instance_files",
        SEARCH_PROJECTS => "curseforge_search_projects",
        SET_API_KEY => "curseforge_set_api_key",
        UPDATE_MANAGED_MODPACK => "curseforge_update_managed_modpack",
        VALIDATE_CREDENTIALS => "curseforge_validate_credentials",
    }

    datapacks => "datapacks" {
        DELETE_DATAPACK => "delete_datapack",
        LIST_DATAPACKS => "list_datapacks",
        SET_DATAPACK_ENABLED => "set_datapack_enabled",
    }

    mcarchive => "mcarchive" {
        GET_GAME_VERSIONS => "mcarchive_get_game_versions",
        SEARCH_MODS => "mcarchive_search_mods",
    }

    metadata => "metadata" {
        GET_GAME_VERSIONS => "metadata_get_game_versions",
        GET_LOADER_VERSIONS => "metadata_get_loader_versions",
    }

    minecraft_skins => "minecraft-skins" {
        ADD_AND_EQUIP_CUSTOM_SKIN => "add_and_equip_custom_skin",
        EQUIP_SKIN => "equip_skin",
        FLUSH_PENDING_SKIN_CHANGE => "flush_pending_skin_change",
        FLUSH_PENDING_SKIN_CHANGE_FOR_PROFILE => "flush_pending_skin_change_for_profile",
        GET_AVAILABLE_CAPES => "get_available_capes",
        GET_AVAILABLE_SKINS => "get_available_skins",
        GET_DRAGGED_SKIN_DATA => "get_dragged_skin_data",
        LOOKUP_PLAYER_SKIN => "lookup_player_skin",
        NORMALIZE_SKIN_TEXTURE => "normalize_skin_texture",
        REMOVE_CUSTOM_SKIN => "remove_custom_skin",
        SAVE_CUSTOM_SKIN => "save_custom_skin",
        SET_CUSTOM_SKIN_ORDER => "set_custom_skin_order",
        UNEQUIP_SKIN => "unequip_skin",
    }

    mod_translation => "mod-translation" {
        ANALYZE => "mod_translation_analyze",
        CANCEL => "mod_translation_cancel",
        CANCEL_ANALYSIS => "mod_translation_cancel_analysis",
        DISMISS_TASK => "mod_translation_dismiss_task",
        GET_TASK => "mod_translation_get_task",
        LIST_TASKS => "mod_translation_list_tasks",
        TRANSLATE => "mod_translation_translate",
    }

    planet_minecraft => "planet-minecraft" {
        CONNECTOR_AVAILABLE => "planet_minecraft_connector_available",
        SEARCH_PROJECTS => "planet_minecraft_search_projects",
    }

    resources => "axolotl-resources" {
        RESOURCE_ANALYSIS_CLOSE => "resource_analysis_close",
        RESOURCE_ANALYSIS_OPEN => "resource_analysis_open",
        RESOURCE_ANALYSIS_UPDATE => "resource_analysis_update",
        RESOURCE_CANCEL => "resource_cancel",
        RESOURCE_CHECK_UPDATES => "resource_check_updates",
        RESOURCE_EXECUTE_HISTORY_CLEANUP => "resource_execute_history_cleanup",
        RESOURCE_EXPORT_CANCEL => "resource_export_cancel",
        RESOURCE_EXPORT_CLOSE => "resource_export_close",
        RESOURCE_EXPORT_EXECUTE => "resource_export_execute",
        RESOURCE_EXPORT_PLAN => "resource_export_plan",
        RESOURCE_EXPORT_QUEUE => "resource_export_queue",
        RESOURCE_EXPORT_REFRESH => "resource_export_refresh",
        RESOURCE_EXPORT_REFRESH_RETRY => "resource_export_refresh_retry",
        RESOURCE_EXPORT_SCAN => "resource_export_scan",
        RESOURCE_FAVORITE_ADD => "resource_favorite_add",
        RESOURCE_FAVORITE_GROUP_CREATE => "resource_favorite_group_create",
        RESOURCE_FAVORITE_GROUP_DELETE => "resource_favorite_group_delete",
        RESOURCE_FAVORITE_GROUP_RENAME => "resource_favorite_group_rename",
        RESOURCE_FAVORITE_GROUP_UPDATE_MEMBERSHIPS => "resource_favorite_group_update_memberships",
        RESOURCE_FAVORITE_GROUPS => "resource_favorite_groups",
        RESOURCE_FAVORITE_REMOVE => "resource_favorite_remove",
        RESOURCE_FAVORITES => "resource_favorites",
        RESOURCE_GET => "resource_get",
        RESOURCE_GET_CHANGELOG => "resource_get_changelog",
        RESOURCE_GET_CONTENT_REFRESH_STATE => "resource_get_content_refresh_state",
        RESOURCE_GET_DESCRIPTION => "resource_get_description",
        RESOURCE_LIST_HISTORY => "resource_list_history",
        RESOURCE_LIST_INSTALLED => "resource_list_installed",
        RESOURCE_LIST_RELEASES => "resource_list_releases",
        RESOURCE_LOCATE => "resource_locate",
        RESOURCE_PREFERRED_SOURCE => "resource_preferred_source",
        RESOURCE_PREPARE_ENTRY_OPERATION => "resource_prepare_entry_operation",
        RESOURCE_PREPARE_HISTORY_CLEANUP => "resource_prepare_history_cleanup",
        RESOURCE_PREPARE_OPERATION => "resource_prepare_operation",
        RESOURCE_PROJECT_DOCUMENTS => "resource_project_documents",
        RESOURCE_PROJECT_PROTOCOL => "resource_project_protocol",
        RESOURCE_QUEUE_PLAN => "resource_queue_plan",
        RESOURCE_RELEASE_DOCUMENTS => "resource_release_documents",
        RESOURCE_REVISE_PLAN => "resource_revise_plan",
        RESOURCE_ROLLBACK => "resource_rollback",
        RESOURCE_SCAN_BACKUPS => "resource_scan_backups",
        RESOURCE_SEARCH => "resource_search",
        RESOURCE_SET_PREFERRED_SOURCE => "resource_set_preferred_source",
        RESOURCE_START_CONTENT_REFRESH => "resource_start_content_refresh",
    }

    schematics => "schematics" {
        GET => "schematics_get",
        INSTALL => "schematics_install",
        LIST => "schematics_list",
    }

    tags => "tags" {
        GET_CATEGORIES => "tags_get_categories",
        GET_DONATION_PLATFORMS => "tags_get_donation_platforms",
        GET_GAME_VERSIONS => "tags_get_game_versions",
        GET_LOADERS => "tags_get_loaders",
        GET_REPORT_TYPES => "tags_get_report_types",
    }

    theme_pack => "theme-pack" {
        CLOSE_PAGE => "theme_pack_close_page",
        EXPORT_SNAPSHOT => "theme_pack_export_snapshot",
        GET => "theme_pack_get",
        INSTALL => "theme_pack_install",
        LIMITS => "theme_pack_limits",
        LIST => "theme_pack_list",
        OPEN_PAGE => "theme_pack_open_page",
        PAGE_BOUNDS => "theme_pack_page_bounds",
        PAGE_REQUEST => "theme_pack_page_request",
        REMOVE => "theme_pack_remove",
        RESOLVE_PAGE => "theme_pack_resolve_page",
    }
}

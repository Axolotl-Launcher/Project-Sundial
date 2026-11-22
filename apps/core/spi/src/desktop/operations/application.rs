// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::DesktopOperation;

operation_groups! {
    ai => "ai" {
        BEGIN_OAUTH => "ai_begin_oauth",
        DISCONNECT_OAUTH => "ai_disconnect_oauth",
        FETCH_MODELS => "ai_fetch_models",
        GET_CATALOG => "ai_get_catalog",
        GET_STATE => "ai_get_state",
        POLL_OAUTH => "ai_poll_oauth",
        REMOVE_MODEL => "ai_remove_model",
        SET_API_KEY => "ai_set_api_key",
        SET_CREDENTIAL => "ai_set_credential",
        TEST_PROVIDER => "ai_test_provider",
        UPDATE_MODEL => "ai_update_model",
        UPDATE_PROVIDER => "ai_update_provider",
        UPDATE_SETTINGS => "ai_update_settings",
    }

    app => "app" {
        ALLOW_SYMLINK_TARGET => "allow_symlink_target",
        ARE_UPDATES_ENABLED => "are_updates_enabled",
        BACKUP_APP_DB_FOR_UPDATE => "backup_app_db_for_update",
        CHECK_APP_UPDATE => "check_app_update",
        CHECK_SYMLINK_CAPABILITY => "check_symlink_capability",
        ENQUEUE_UPDATE_FOR_INSTALLATION => "enqueue_update_for_installation",
        EXIT_APP => "exit_app",
        FRONTEND_READY => "frontend_ready",
        GET_UPDATE_CHANNEL => "get_update_channel",
        GET_UPDATE_PREFERENCES => "get_update_preferences",
        GET_UPDATE_SIZE => "get_update_size",
        INITIALIZE_STATE => "initialize_state",
        IS_DEV => "is_dev",
        IS_ELEVATED => "is_elevated",
        IS_PORTABLE_MODE => "is_portable_mode",
        LIGHTWEIGHT_MODE_ENTER => "lightweight_mode_enter",
        LIGHTWEIGHT_MODE_FRONTEND_READY => "lightweight_mode_frontend_ready",
        LIGHTWEIGHT_MODE_SET_ROUTE => "lightweight_mode_set_route",
        REMOVE_ENQUEUED_UPDATE => "remove_enqueued_update",
        RESTART_APP => "restart_app",
        SET_DISCORD_ACTIVITY => "set_discord_activity",
        SET_RESTART_AFTER_PENDING_UPDATE => "set_restart_after_pending_update",
        SET_TRANSPARENT_WINDOW_FRAME => "set_transparent_window_frame",
        SET_UPDATE_CHANNEL => "set_update_channel",
        SET_UPDATE_PREFERENCES => "set_update_preferences",
        SHOW_WINDOW => "show_window",
        UPDATE_TRAFFIC_LIGHTS_ANCHOR => "update_traffic_lights_anchor",
    }

    drop => "drop" {
        CLASSIFY => "drop_classify",
        CLASSIFY_EXTRACT => "drop_classify_extract",
        DETECT_FILE_LOCK => "drop_detect_file_lock",
        EXTRACT_MOD_METADATA => "drop_extract_mod_metadata",
        EXTRACT_ZIP_TO_TEMP => "drop_extract_zip_to_temp",
        LOOKUP_MOD_HASH => "drop_lookup_mod_hash",
        REMOVE_TEMP_DIR => "drop_remove_temp_dir",
        RESOLVE_GAMEDIRS => "drop_resolve_gamedirs",
        SCAN_LAUNCHER_INSTANCES => "drop_scan_launcher_instances",
    }

    files => "files" {
        BACKGROUND_CONVERT_ANIMATED => "background_convert_animated",
        FILE_EXTRACT_ZIP => "file_extract_zip",
        FILE_READ_DRAGGED_FILE => "file_read_dragged_file",
        FILE_SAVE_AS => "file_save_as",
        SCREENSHOT_THUMBNAIL => "screenshot_thumbnail",
        STUDIO_READ_BINARY => "studio_read_binary",
        STUDIO_READ_TEXT => "studio_read_text",
        STUDIO_TRASH => "studio_trash",
        STUDIO_WATCH_REGISTER => "studio_watch_register",
        STUDIO_WATCH_UNREGISTER => "studio_watch_unregister",
        STUDIO_WRITE_BINARY => "studio_write_binary",
    }

    fonts => "fonts" {
        GET_SYSTEM_FONTS => "fonts_get_system_fonts",
    }

    mcp => "mcp" {
        CONFIGURE => "mcp_configure",
        DISCONNECT => "mcp_disconnect",
        GET_STATE => "mcp_get_state",
        RESPOND => "mcp_respond",
        UI_CLAIM => "mcp_ui_claim",
        UI_RESPOND => "mcp_ui_respond",
    }

    multiplayer => "multiplayer" {
        DOWNLOAD_HONGSHI => "multiplayer_download_hongshi",
        GET_DETECTED_PORTS => "multiplayer_get_detected_ports",
        GET_NODES => "multiplayer_get_nodes",
        GET_PLAYER_NAME => "multiplayer_get_player_name",
        GET_STATE => "multiplayer_get_state",
        HOST => "multiplayer_host",
        JOIN => "multiplayer_join",
        OPEN_HONGSHI_LOGS => "multiplayer_open_hongshi_logs",
        PREPARE_TERRACOTTA => "multiplayer_prepare_terracotta",
        RESET => "multiplayer_reset",
        STOP => "multiplayer_stop",
        SWITCH_PROVIDER => "multiplayer_switch_provider",
    }

    observability => "observability" {
        FLUSH => "observability_flush",
        RECORD => "observability_record",
        STATUS => "observability_status",
    }

    observability_mcp => "observability-mcp" {
        CONFIGURE => "observability_mcp_configure",
        GET_STATE => "observability_mcp_get_state",
    }

    schematic_preview => "schematic-preview" {
        APPLY_EDITS => "schematic_preview_apply_edits",
        BLOCK_INFO => "schematic_preview_block_info",
        CANCEL => "schematic_preview_cancel",
        CLOSE => "schematic_preview_close",
        EXPORT_LITEMATIC => "schematic_preview_export_litematic",
        EXPORT_SPONGE => "schematic_preview_export_sponge",
        LIST_INSTANCE_FILES => "schematic_preview_list_instance_files",
        OPEN => "schematic_preview_open",
        READ_CHUNK => "schematic_preview_read_chunk",
        TRANSFORM => "schematic_preview_transform",
    }

    seed_map => "seed-map" {
        BIOME_AT => "seed_map_biome_at",
        CANCEL_ORE_SCAN => "seed_map_cancel_ore_scan",
        CANCEL_UNDERGROUND => "seed_map_cancel_underground",
        CONTAINER_LOOT => "seed_map_container_loot",
        FIND_FEATURES => "seed_map_find_features",
        PROFILES => "seed_map_profiles",
        READ_LEVEL_DAT => "seed_map_read_level_dat",
        RENDER_TILE => "seed_map_render_tile",
        SCAN_ORES => "seed_map_scan_ores",
        SPAWN => "seed_map_spawn",
        STRUCTURE_DETAILS => "seed_map_structure_details",
        UNDERGROUND_VOLUME => "seed_map_underground_volume",
    }

    servers => "servers" {
        CLEAR_LOG => "servers_clear_log",
        CREATE => "servers_create",
        DELETE => "servers_delete",
        DOWNLOAD_FILE => "servers_download_file",
        GET => "servers_get",
        GET_CONSOLE_DELTA => "servers_get_console_delta",
        GET_LOG_BUFFER => "servers_get_log_buffer",
        GET_LOG_DELTA => "servers_get_log_delta",
        INSTALL_FORGE => "servers_install_forge",
        INSTALL_MODPACK => "servers_install_modpack",
        INSTALL_NEOFORGE => "servers_install_neoforge",
        IS_LOCAL_ADDRESS => "servers_is_local_address",
        KILL => "servers_kill",
        KILL_PORT_PROCESS => "servers_kill_port_process",
        LIST => "servers_list",
        PORT_PROCESS => "servers_port_process",
        READ_FILE => "servers_read_file",
        RESIZE_CONSOLE => "servers_resize_console",
        SEND_CONSOLE_INPUT => "servers_send_console_input",
        SET_ICON => "servers_set_icon",
        SET_LINKED_WORLD => "servers_set_linked_world",
        START => "servers_start",
        STOP => "servers_stop",
        UPDATE_SETTINGS => "servers_update_settings",
        WAIT_UNTIL_READY => "servers_wait_until_ready",
        WRITE_FILE => "servers_write_file",
    }

    settings => "settings" {
        CANCEL_DIRECTORY_CHANGE => "cancel_directory_change",
        DISCORD_RPC_SET => "discord_rpc_set",
        NETWORK_TEST => "network_test",
        NETWORK_TEST_CANCEL => "network_test_cancel",
        PRIVACY_GET => "privacy_get",
        PRIVACY_SET => "privacy_set",
        PROXY_GET => "proxy_get",
        PROXY_SET => "proxy_set",
        PROXY_TEST => "proxy_test",
        EXPORT => "settings_export",
        GET => "settings_get",
        IMPORT => "settings_import",
        IMPORT_SCAN => "settings_import_scan",
        LAYERS => "settings_layers",
        OVERRIDE_CLEAR => "settings_override_clear",
        PATCH => "settings_patch",
        SANITISE => "settings_sanitise",
        TELEMETRY_SET => "telemetry_set",
    }

    shortcuts => "shortcuts" {
        CREATE_INSTANCE_SHORTCUT => "create_instance_shortcut",
    }

    system_accent => "system-accent" {
        COLOR => "system_accent_color",
    }

    telemetry => "telemetry" {
        NOTIFY_ONLINE => "notify_online",
    }

    terracotta => "terracotta" {
        CHECK_FOR_UPDATE => "terracotta_check_for_update",
        DOWNLOAD => "terracotta_download",
        GET_DIAGNOSTIC_REPORT => "terracotta_get_diagnostic_report",
        GET_PLATFORM_KEY => "terracotta_get_platform_key",
        GET_PLAYER_NAME => "terracotta_get_player_name",
        GET_STATE => "terracotta_get_state",
        HOST => "terracotta_host",
        JOIN => "terracotta_join",
        RESET => "terracotta_reset",
        START => "terracotta_start",
        UPDATE => "terracotta_update",
    }

    third_party_accounts => "third-party-accounts" {
        BEGIN_AUTHORIZATION => "begin_authorization",
        CANCEL_AUTHORIZATION => "cancel_authorization",
        CHECK_CONNECTION => "check_connection",
        DISCONNECT => "disconnect",
        GET_ACCOUNT => "get_account",
    }

    translation => "translation" {
        CLEAR_CACHE => "translation_clear_cache",
        GET_SETTINGS => "translation_get_settings",
        GOOGLE_IP_POOL_SIZE => "translation_google_ip_pool_size",
        TEST_PROVIDER => "translation_test_provider",
        TRANSLATE => "translation_translate",
        UPDATE_SETTINGS => "translation_update_settings",
    }

    utils => "utils" {
        EXPORT_ERROR_LOGS => "export_error_logs",
        EXPORT_LAUNCHER_LOGS => "export_launcher_logs",
        GET_MINECRAFT_NEWS => "get_minecraft_news",
        GET_OPENING_COMMAND => "get_opening_command",
        GET_OS => "get_os",
        HIGHLIGHT_IN_FOLDER => "highlight_in_folder",
        IS_NETWORK_METERED => "is_network_metered",
        OPEN_PATH => "open_path",
        PROGRESS_BARS_LIST => "progress_bars_list",
        SHOULD_DISABLE_MOUSEOVER => "should_disable_mouseover",
        SHOW_APP_DB_BACKUPS_FOLDER => "show_app_db_backups_folder",
        SHOW_LAUNCHER_LOGS_FOLDER => "show_launcher_logs_folder",
    }
}

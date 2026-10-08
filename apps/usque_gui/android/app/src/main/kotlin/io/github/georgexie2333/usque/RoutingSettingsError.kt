package io.github.georgexie2333.usque

/** Only fixed validation codes and UUIDs may cross the settings error bridge. */
internal object RoutingSettingsError {
    private const val ID = "[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}"
    private val token =
        Regex(
            "\\b(?:ROUTING_RULE_CONFLICT:$ID:$ID|ROUTING_RULE_INVALID:$ID|ROUTING_RULE_LIMIT|ROUTING_UPGRADE_REQUIRED)(?![a-zA-Z0-9:_-])",
        )

    fun fromNative(message: String?): String? = message?.takeIf { it.length <= 4096 }?.let { token.find(it)?.value }

    fun fromWire(message: String?): String? = message?.takeIf { it.length <= 128 && token.matches(it) }

    fun nativeFailure(message: String?): String =
        fromNative(message) ?: if (message?.contains("NETWORK_SETTINGS_SAVE_FAILED") == true) {
            "NETWORK_SETTINGS_SAVE_FAILED"
        } else {
            "NETWORK_SETTINGS_UNCONFIRMED"
        }
}

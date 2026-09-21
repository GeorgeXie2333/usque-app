package io.github.georgexie2333.usque

import org.json.JSONObject

/** Passwords have one owner: the shared network, independent of WARP accounts. */
internal object SharedProxyCredentials {
    const val SHARED_ID = "shared-network"
    private val record = SecureIdentityStore.Record.PROXY_PASSWORD

    fun accountIds(catalog: JSONObject): Set<String> =
        buildSet {
            val profiles = catalog.optJSONArray("profiles")
            if (profiles != null) {
                for (index in 0 until profiles.length()) add(profiles.getJSONObject(index).getString("id"))
            }
            val pending = catalog.optJSONArray("pending_identity_deletions")
            if (pending != null) for (index in 0 until pending.length()) add(pending.getString(index))
        }

    fun read(
        store: AndroidEngineMethodHandler.IdentityStore,
        catalog: JSONObject,
    ): ByteArray? = store.withProxyLock { readLocked(store, catalog) }

    fun <T> withCurrent(
        store: AndroidEngineMethodHandler.IdentityStore,
        loadCatalog: () -> JSONObject,
        apply: (JSONObject, ByteArray) -> T,
    ): T =
        store.withProxyLock {
            val catalog = loadCatalog()
            val password = readLocked(store, catalog) ?: ByteArray(0)
            try {
                apply(catalog, password)
            } finally {
                password.fill(0)
            }
        }

    private fun readLocked(
        store: AndroidEngineMethodHandler.IdentityStore,
        catalog: JSONObject,
    ): ByteArray? {
        val shared = store.get(SHARED_ID, record)
        if (shared != null) return shared
        var candidate: ByteArray? = null
        return try {
            val ids = accountIds(catalog)
            for (id in ids) {
                val value = store.get(id, record) ?: continue
                try {
                    check(candidate == null || candidate.contentEquals(value)) {
                        "Conflicting legacy proxy passwords; save new shared credentials"
                    }
                    if (candidate == null) candidate = value.copyOf()
                } finally {
                    value.fill(0)
                }
            }
            candidate?.let { store.put(SHARED_ID, record, it) }
            if (candidate != null) for (id in ids) store.delete(id, record)
            candidate?.copyOf()
        } finally {
            candidate?.fill(0)
        }
    }

    fun save(
        store: AndroidEngineMethodHandler.IdentityStore,
        catalog: JSONObject,
        password: ByteArray,
        persistUsername: () -> Unit,
    ) = store.withProxyLock {
        // A new explicit credential resolves conflicting legacy records. Never
        // enable a username before its password is available to a cold start.
        if (password.isNotEmpty()) store.put(SHARED_ID, record, password)
        persistUsername()
        for (id in accountIds(catalog)) store.delete(id, record)
        if (password.isEmpty()) store.delete(SHARED_ID, record)
    }
}

package io.github.georgexie2333.usque

/** Main-owner proof of the profile actually published with a specific TUN. */
internal class EstablishedTunProtection {
    private var descriptor: Any? = null
    private var profile: String? = null

    fun published(
        ownedDescriptor: Any,
        previousDescriptor: Any?,
        requestedProfile: String,
    ) {
        // Reusing an FD does not establish the requested settings on it.
        // Keep the original proof until native application is confirmed.
        if (ownedDescriptor !== previousDescriptor) established(ownedDescriptor, requestedProfile)
    }

    fun established(
        ownedDescriptor: Any,
        establishedProfile: String,
    ) {
        descriptor = ownedDescriptor
        profile = establishedProfile
    }

    fun profileFor(ownedDescriptor: Any?): String? =
        if (ownedDescriptor != null && ownedDescriptor === descriptor) profile else null

    fun closed(closedDescriptor: Any?) {
        if (closedDescriptor === descriptor) {
            descriptor = null
            profile = null
        }
    }
}

package io.github.georgexie2333.usque

import android.app.Application

class UsqueApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        UiProcessReclaimer.install(this)
    }
}

package com.remora.plugin

import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory

class RemoraStatusBarFactory : StatusBarWidgetFactory {
    override fun getId(): String = "RemoraStatusBar"
    override fun getDisplayName(): String = "Remora"
    override fun createWidget(project: Project): StatusBarWidget = RemoraStatusBarWidget()
}

class RemoraStatusBarWidget : StatusBarWidget, StatusBarWidget.TextPresentation {
    override fun ID(): String = "RemoraStatusBar"
    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this
    override fun getText(): String = "Remora: disconnected"
    override fun getTooltipText(): String = "Remora session status"
    override fun getAlignment(): Float = 0f
    override fun install(statusBar: StatusBar) {}
    override fun dispose() {}
}

plugins {
    alias(libs.plugins.android.application) apply false
    // AGP 9'un yerleşik Kotlin desteği bu sürümü kullanır; uygulamaya ayrıca uygulanmaz.
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
}

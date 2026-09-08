allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

val newBuildDir: Directory =
    rootProject.layout.buildDirectory
        .dir("../../build")
        .get()
rootProject.layout.buildDirectory.value(newBuildDir)

subprojects {
    val newSubprojectBuildDir: Directory = newBuildDir.dir(project.name)
    project.layout.buildDirectory.value(newSubprojectBuildDir)
}
// Third-party Flutter plugin modules that hardcode an old `compileSdk` in their
// own build.gradle (e.g. file_picker 8.3.7 pins 34) fail `checkDebugAarMetadata`
// against the Flutter 3.47 AARs, which declare a compileSdk-36 floor. Raise any
// such module to the app's compileSdk. This is COMPILE-TIME ONLY — each module
// keeps its own minSdk/targetSdk, so runtime behavior is unchanged.
subprojects {
    afterEvaluate {
        val androidExt = extensions.findByName("android") ?: return@afterEvaluate
        androidExt.withGroovyBuilder {
            val current = ("getCompileSdkVersion"() as? String)
                ?.removePrefix("android-")
                ?.toIntOrNull()
            if (current != null && current < 36) {
                "compileSdkVersion"(36)
            }
        }
    }
}

subprojects {
    project.evaluationDependsOn(":app")
}

tasks.register<Delete>("clean") {
    delete(rootProject.layout.buildDirectory)
}

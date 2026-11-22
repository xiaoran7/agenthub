# Preserve source file & line numbers in release stack traces.
-keepattributes SourceFile,LineNumberTable,Signature,InnerClasses,EnclosingMethod

# Keep enum entries used in persisted wire/UI state.
-keepclassmembers enum com.agentplatform.mobile.** {
    public static **[] values();
    public static ** valueOf(java.lang.String);
}

# Strip verbose debug logs in release builds.
-assumenosideeffects class android.util.Log {
    public static int d(...);
    public static int v(...);
}

fn main() {
    #[cfg(target_os = "windows")]
    {
        // Debug 模式下用默认 CONSOLE 子系统（main.rs 不含 windows_subsystem = "windows"）
        // Release 模式强制 WINDOWS,6.01 兼容 Win7
        #[cfg(not(debug_assertions))]
        println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS,6.01");
        println!("cargo:rustc-link-arg=/DELAYLOAD:api-ms-win-core-synch-l1-2-0.dll");
        println!("cargo:rustc-link-arg=/DELAYLOAD:combase.dll");
        println!("cargo:rustc-link-arg=/DELAYLOAD:bcryptprimitives.dll");
        println!("cargo:rustc-link-arg=/DELAYLOAD:api-ms-win-core-winrt-l1-1-0.dll");
        println!("cargo:rustc-link-arg=/DELAYLOAD:api-ms-win-core-winrt-string-l1-1-0.dll");
        // kernel32.dll 中 Win8+ 函数的延迟加载
        println!("cargo:rustc-link-arg=/DELAYLOAD:kernel32.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");

        // 使用 .def 文件强制导出 stub 函数
        println!("cargo:rustc-link-arg=/DEF:win7_stubs.def");

        // /FORCE:MULTIPLE — libstd / windows crate 的 .rlib 内嵌了这些 DLL
        // 的 import library thunk 符号，与 win7_stubs.rs 的 #[no_mangle] 定义冲突。
        // object file（.o）在命令行中排在 .rlib 前面，MULTIPLE 模式下先出现者赢，
        // 因此我们的 stub 会覆盖 .rlib 中的 import thunk，避免产生 IAT 导入。
        println!("cargo:rustc-link-arg=/FORCE:MULTIPLE");
    }

    tauri_build::build()
}

//! Compila as funções matemáticas do musl 1.2.3 (musl-math/) e as liga no lugar da libm do sistema.
//!
//! O binário de referência deep-filter v0.5.6 é estático com musl; com a libm do sistema
//! (glibc, macOS, Windows) a saída do DeepFilterNet difere em 1 LSB (16-bit) em ~0,04% das
//! amostras. Usando as mesmas funções (expf, logf, powf, sinf...) o resultado fica bit a bit igual.

fn main() {
    println!("cargo:rerun-if-changed=musl-math/src");
    let fontes: Vec<_> = std::fs::read_dir("musl-math/src")
        .expect("musl-math/src")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "c"))
        .collect();
    let mut b = cc::Build::new();
    b.files(&fontes)
        .include("musl-math/src")
        .opt_level(2)
        .warnings(false)
        .cargo_metadata(false);
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    // No Windows, preferir o clang-cl (vem com o Visual Studio e nos runners do GitHub)
    if msvc && std::process::Command::new("clang-cl").arg("--version").output().is_ok() {
        b.compiler("clang-cl");
    }
    if msvc {
        // o math.h da UCRT só define M_PI_2 (usado em sinf/cosf/tanf) com isto
        b.define("_USE_MATH_DEFINES", None);
        // sem dllimport nas declarações do math.h: as chamadas internas (expf, expm1...) usam as
        // nossas definições em vez de puxar o ucrt.lib (LNK2005 "already defined")
        b.define("_ACRTIMP", Some(""));
    }
    let compilador = b.get_compiler();
    if compilador.is_like_clang_cl() {
        b.flag("/clang:-fno-builtin").flag("/clang:-ffp-contract=off").flag("/clang:-fno-fast-math");
    } else if compilador.is_like_msvc() {
        // cl.exe: sem /arch:AVX2 não há FMA; /fp:precise não reordena operações
        b.flag("/fp:precise");
    } else {
        // sem FMA implícito nem substituições de builtins: mesma aritmética do musl compilado
        b.flag("-fno-builtin").flag("-ffp-contract=off").flag("-fno-fast-math");
    }
    b.compile("muslmath");
    let saida = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rustc-link-search=native={saida}");
    // whole-archive: as definições entram no executável e têm prioridade sobre a libm do sistema
    println!("cargo:rustc-link-lib=static:+whole-archive=muslmath");
}

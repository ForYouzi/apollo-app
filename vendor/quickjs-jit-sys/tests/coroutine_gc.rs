#![cfg(all(feature = "jit-abi", unix))]

//! APOLLO (#20): patches/0033-coroutine-varref-gc.patch, quickjs-ng 7955cfd.
//! A suspended coroutine reachable only through a closure over one of its
//! locals was collected while live; resuming it read freed memory. A mod task
//! that parks itself on `await new Promise(r => wake = r)` with `wake` called
//! from a timer is exactly that shape, and crashed Apollo in `js_free_value_rt`.

#[path = "../build_support/patch.rs"]
mod patch;

use std::{fs, path::PathBuf, process::Command};

#[test]
fn suspended_coroutines_survive_collection() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("mktemp")
        .args(["-d", "/tmp/quickjs-coroutine-gc-XXXXXX"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let scratch = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    for file in patch::BASELINE_FILES {
        fs::copy(manifest.join("quickjs").join(file), scratch.join(file)).unwrap();
    }
    patch::apply_patch_set(&scratch, &manifest.join("patches")).unwrap();
    for file in [
        "quickjs-jit-opcodes.generated.h",
        "quickjs-jit-helpers.generated.h",
    ] {
        fs::copy(PathBuf::from(env!("OUT_DIR")).join(file), scratch.join(file)).unwrap();
    }
    fs::write(scratch.join("coroutine-gc.c"), FIXTURE).unwrap();
    // AddressSanitizer turns the use-after-free into a failure; without it the
    // stale read may go unnoticed, so the fixture also checks the values.
    let asan = Command::new("clang").arg("--version").output().is_ok_and(|o| o.status.success());
    let mut compiler = Command::new(if asan { "clang" } else { "cc" });
    compiler.current_dir(&scratch);
    if asan {
        compiler.args(["-fsanitize=address", "-fno-sanitize-recover=all", "-fno-omit-frame-pointer"]);
    }
    let compile = compiler
        .args([
            "-std=gnu11",
            "-O1",
            "-g",
            "-w",
            "-D_GNU_SOURCE",
            "-DNDEBUG",
            "-DCONFIG_JIT_ABI=1",
            "coroutine-gc.c",
            "quickjs.c",
            "libregexp.c",
            "libunicode.c",
            "dtoa.c",
            "-lm",
            "-lpthread",
            "-o",
            "coroutine-gc",
        ])
        .output()
        .unwrap();
    assert!(compile.status.success(), "{}", String::from_utf8_lossy(&compile.stderr));
    let run = Command::new(scratch.join("coroutine-gc"))
        .env("ASAN_OPTIONS", "detect_leaks=0")
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "a collected coroutine was resumed in {}:\n{}\n{}",
        scratch.display(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    fs::remove_dir_all(scratch).unwrap();
}

const FIXTURE: &str = r#"
#include "quickjs.h"
#include <stdio.h>
#include <string.h>

static JSValue gc_fn(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    JS_RunGC(JS_GetRuntime(ctx));
    return JS_UNDEFINED;
}

/* Runs the queued promise jobs, as the host's event loop does between timers. */
static JSValue drain_fn(JSContext *ctx, JSValueConst this_val, int argc, JSValueConst *argv) {
    JSContext *job;
    while (JS_ExecutePendingJob(JS_GetRuntime(ctx), &job) > 0) {}
    return JS_UNDEFINED;
}

static int check(const char *label, const char *src, const char *want) {
    JSRuntime *rt = JS_NewRuntime();
    JSContext *ctx = JS_NewContext(rt);
    JSValue global = JS_GetGlobalObject(ctx);
    JS_SetPropertyStr(ctx, global, "__gc", JS_NewCFunction(ctx, gc_fn, "__gc", 0));
    JS_SetPropertyStr(ctx, global, "__drain", JS_NewCFunction(ctx, drain_fn, "__drain", 0));
    JS_FreeValue(ctx, global);
    JSValue result = JS_Eval(ctx, src, strlen(src), label, JS_EVAL_TYPE_GLOBAL);
    JSContext *job;
    while (JS_ExecutePendingJob(rt, &job) > 0) {}
    if (JS_IsException(result)) {
        JSValue error = JS_GetException(ctx);
        result = error;
    }
    const char *got = JS_ToCString(ctx, result);
    int ok = got && strcmp(got, want) == 0;
    printf("%s: %s (want %s)\n", label, got ? got : "?", want);
    if (got) JS_FreeCString(ctx, got);
    JS_FreeValue(ctx, result);
    JS_FreeContext(ctx);
    JS_FreeRuntime(rt);
    return ok;
}

int main(void) {
    int ok = 1;
    ok &= check("async closure",
        "globalThis.leaked=null;(function(){async function step(){const d=Promise.withResolvers();"
        "globalThis.leaked=()=>d;await d.promise;}step();})();__gc();typeof leaked().resolve", "function");
    ok &= check("generator closure",
        "globalThis.leaked=null;(function(){let g;function* gen(){const o={};o.g=g;globalThis.leaked=()=>o;yield;}"
        "g=gen();g.next();g=null;})();__gc();String(leaked().g!=null)", "true");
    ok &= check("mapped arguments",
        "globalThis.leaked=null;(function(){function* gen(a){globalThis.leaked=()=>arguments;yield;}"
        "gen({v:1}).next();})();__gc();String(leaked()[0].v)", "1");
    ok &= check("async generator closure",
        "globalThis.leaked=null;(function(){async function* ag(){const o={k:7};globalThis.leaked=()=>o;"
        "await new Promise(()=>{});yield 1;}ag().next();})();__gc();String(leaked().k)", "7");
    /* The Apollo mod shape: a long-lived task parked on a promise whose resolver
       only a timer closure (over the task's own local) can reach. */
    ok &= check("timer-woken task",
        "globalThis.n=0;globalThis.wakeAll=null;(function(){async function loop(){let wake=null;"
        "wakeAll=()=>{const w=wake;wake=null;if(w)w({t:1});};for(let i=0;i<50;i++){"
        "const found=[{k:1},{k:2}].find(o=>o.k===2);const r=await new Promise(res=>{wake=res;});n+=r.t+found.k;}}"
        "loop();})();for(let i=0;i<50;i++){__gc();wakeAll();__drain();}String(n)", "150");
    ok &= check("for-of resumed with a rejection",
        "globalThis.rej=null;globalThis.n=0;(async function(){for(;;){try{for(const a of [[1],[2]]){"
        "await new Promise((_,j)=>{rej=j;});}}catch(e){n++;}}})();"
        "for(let i=0;i<50;i++){__gc();__drain();if(rej)rej(new Error('x'));__drain();}String(n)", "50");
    ok &= check("closure outliving its coroutine",
        "globalThis.f=null;(async function(){let x={v:3};f=()=>x;await 0;})();__gc();String(f().v)", "3");
    return ok ? 0 : 1;
}
"#;

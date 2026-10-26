// P/Invoke wrapper for neat_adblock.dll. NOT compiled or run yet (no .NET SDK in the environment
// this was written in): treat it as a starting point and expect to fix small things.
//
// Target: .NET 6+ (WinUI 3). Enable <AllowUnsafeBlocks> is NOT required.

using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Neat.Adblock;

public enum AdblockAction { Allow = 0, Block = 1, Redirect = 2 }

/// <param name="Redirect">Action == Redirect: replacement resource (a data: URL) to serve instead.</param>
/// <param name="RewrittenUrl">Action == Allow: when not null, load this URL instead ($removeparam).</param>
public readonly record struct CheckResult(AdblockAction Action, string? Redirect, string? RewrittenUrl);

public sealed class AdblockException : Exception
{
    public AdblockException(string message) : base(message) { }
}

internal static class Native
{
    private const string Lib = "neat_adblock";

    [StructLayout(LayoutKind.Sequential)]
    public struct CheckResultNative
    {
        public int Action;
        public IntPtr Redirect;
        public IntPtr RewrittenUrl;
    }

    [DllImport(Lib)] public static extern uint neat_abi_version();

    [DllImport(Lib)]
    public static extern IntPtr neat_engine_create(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string dir, out IntPtr error);

    [DllImport(Lib)] public static extern void neat_engine_free(IntPtr engine);
    [DllImport(Lib)] public static extern void neat_string_free(IntPtr s);

    [DllImport(Lib)]
    public static extern int neat_check_request(
        IntPtr engine,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string url,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string sourceUrl,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string requestType,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string? method,
        out CheckResultNative result);

    [DllImport(Lib)] public static extern void neat_check_result_free(ref CheckResultNative result);

    [DllImport(Lib)]
    public static extern int neat_check_popup(
        IntPtr engine,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string targetUrl,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string openerUrl);

    [DllImport(Lib)]
    public static extern int neat_check_document(
        IntPtr engine,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string url);

    [DllImport(Lib)]
    public static extern int neat_get_csp(
        IntPtr engine,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string url,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string sourceUrl,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string requestType,
        out IntPtr csp);

    [DllImport(Lib)]
    public static extern int neat_cosmetic_resources(
        IntPtr engine, [MarshalAs(UnmanagedType.LPUTF8Str)] string url, out IntPtr json);

    [DllImport(Lib)]
    public static extern int neat_hidden_class_id_selectors(
        IntPtr engine,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string classesJson,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string idsJson,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string exceptionsJson,
        out IntPtr json);
}

/// <summary>
/// One loaded filter release. Thread-safe. Dispose() is safe while other threads are still inside
/// a call: the native engine is only freed once the last in-flight call has returned.
/// </summary>
public sealed class AdblockEngine : IDisposable
{
    public const uint ExpectedAbiVersion = 1;

    private sealed class Handle : SafeHandleZeroOrMinusOneIsInvalid
    {
        public Handle() : base(true) { }
        protected override bool ReleaseHandle() { Native.neat_engine_free(handle); return true; }
        public void Set(IntPtr p) => SetHandle(p);
    }

    private readonly Handle _handle;

    private AdblockEngine(Handle h) => _handle = h;

    /// <summary>Builds an engine from a directory containing a neat-filters release.
    /// Call it off the UI thread.</summary>
    public static AdblockEngine Create(string releaseDirectory)
    {
        if (Native.neat_abi_version() != ExpectedAbiVersion)
            throw new AdblockException($"neat_adblock ABI {Native.neat_abi_version()} != expected {ExpectedAbiVersion}");

        var p = Native.neat_engine_create(releaseDirectory, out var err);
        if (p == IntPtr.Zero)
            throw new AdblockException(TakeString(err) ?? "unknown error creating the engine");

        var h = new Handle();
        h.Set(p);
        return new AdblockEngine(h);
    }

    public void Dispose() => _handle.Dispose();

    // ---- calls --------------------------------------------------------------------------

    /// <param name="requestType">document, subdocument, stylesheet, image, media, font, script,
    /// xmlhttprequest, ping, websocket, object, other</param>
    public CheckResult Check(string url, string sourceUrl, string requestType, string? method = null)
    {
        bool added = false;
        try
        {
            _handle.DangerousAddRef(ref added);
            var rc = Native.neat_check_request(_handle.DangerousGetHandle(), url, sourceUrl, requestType, method, out var r);
            try
            {
                // Bad URLs and similar are treated as "allow": never break a page over a parse error.
                if (rc != 0) return new CheckResult(AdblockAction.Allow, null, null);
                return new CheckResult(
                    (AdblockAction)r.Action,
                    r.Redirect == IntPtr.Zero ? null : Marshal.PtrToStringUTF8(r.Redirect),
                    r.RewrittenUrl == IntPtr.Zero ? null : Marshal.PtrToStringUTF8(r.RewrittenUrl));
            }
            finally { Native.neat_check_result_free(ref r); }
        }
        finally { if (added) _handle.DangerousRelease(); }
    }

    /// <summary>For CoreWebView2.NewWindowRequested. True = cancel the new window.</summary>
    public bool ShouldBlockPopup(string targetUrl, string openerUrl) =>
        With(h => Native.neat_check_popup(h, targetUrl, openerUrl)) == 1;

    /// <summary>For a top-level navigation. True = the page itself is blocked ($document / $all rules only).</summary>
    public bool ShouldBlockDocument(string url) =>
        With(h => Native.neat_check_document(h, url)) == 1;

    /// <summary>Extra CSP directives to append to a document/subdocument response, or null.</summary>
    public string? GetCsp(string url, string sourceUrl, string requestType)
    {
        IntPtr csp = IntPtr.Zero;
        var rc = With(h => Native.neat_get_csp(h, url, sourceUrl, requestType, out csp));
        return rc == 1 ? TakeString(csp) : null;
    }

    /// <summary>JSON: hide_selectors, procedural_actions, exceptions, injected_script, generichide.</summary>
    public string CosmeticResourcesJson(string url)
    {
        IntPtr json = IntPtr.Zero;
        var rc = With(h => Native.neat_cosmetic_resources(h, url, out json));
        return rc == 0 ? TakeString(json) ?? "{}" : "{}";
    }

    /// <summary>Arguments and result are JSON arrays of strings.</summary>
    public string HiddenClassIdSelectorsJson(string classesJson, string idsJson, string exceptionsJson)
    {
        IntPtr json = IntPtr.Zero;
        var rc = With(h => Native.neat_hidden_class_id_selectors(h, classesJson, idsJson, exceptionsJson, out json));
        return rc == 0 ? TakeString(json) ?? "[]" : "[]";
    }

    // ---- helpers ------------------------------------------------------------------------

    private int With(Func<IntPtr, int> call)
    {
        bool added = false;
        try
        {
            _handle.DangerousAddRef(ref added);
            return call(_handle.DangerousGetHandle());
        }
        finally { if (added) _handle.DangerousRelease(); }
    }

    private static string? TakeString(IntPtr p)
    {
        if (p == IntPtr.Zero) return null;
        try { return Marshal.PtrToStringUTF8(p); }
        finally { Native.neat_string_free(p); }
    }
}

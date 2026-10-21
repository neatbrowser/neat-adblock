// NOT compiled or run yet. Maps WebView2 resource contexts to adblock request types.

using Microsoft.Web.WebView2.Core;

namespace Neat.Adblock;

public static class WebView2Mapping
{
    /// <param name="isMainFrameNavigation">true when the request is the top-level page load.
    /// WebResourceRequested does not say which frame a Document request belongs to, so the
    /// caller has to work that out (e.g. from NavigationStarting / the frame's URL).</param>
    public static string RequestType(CoreWebView2WebResourceContext context, bool isMainFrameNavigation = false) =>
        context switch
        {
            CoreWebView2WebResourceContext.Document => isMainFrameNavigation ? "document" : "subdocument",
            CoreWebView2WebResourceContext.Stylesheet => "stylesheet",
            CoreWebView2WebResourceContext.Image => "image",
            CoreWebView2WebResourceContext.Media => "media",
            CoreWebView2WebResourceContext.Font => "font",
            CoreWebView2WebResourceContext.Script => "script",
            CoreWebView2WebResourceContext.XmlHttpRequest => "xmlhttprequest",
            CoreWebView2WebResourceContext.Fetch => "xmlhttprequest",
            CoreWebView2WebResourceContext.Websocket => "websocket",
            CoreWebView2WebResourceContext.Ping => "ping",
            CoreWebView2WebResourceContext.CspViolationReport => "ping",
            _ => "other",
        };
}

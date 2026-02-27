from playwright.sync_api import sync_playwright
import os

def generate_mock_html():
    html_content = """
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>Visual Editor Verification</title>
        <script src="https://cdn.tailwindcss.com"></script>
        <script src="https://unpkg.com/lucide@latest"></script>
    </head>
    <body class="bg-slate-900 text-white p-10">
        <h1 class="text-2xl font-bold mb-6">Visual Editor Component Verification</h1>

        <form id="tplForm">
            <div id="editor-container" class="max-w-2xl mx-auto bg-slate-800 p-6 rounded-xl border border-slate-700">
                <!-- Visual Editor Partial -->
<div class="visual-editor-wrapper">
    <!-- Editor Mode Toggle -->
    <div class="flex items-center justify-end mb-4">
        <label class="inline-flex items-center cursor-pointer">
            <span class="mr-3 text-sm font-medium text-slate-300">Visual</span>
            <input type="checkbox" class="editor-mode-toggle sr-only peer">
            <div
                class="relative w-11 h-6 bg-slate-700 peer-focus:outline-none peer-focus:ring-4 peer-focus:ring-emerald-800 rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-5 after:w-5 after:transition-all peer-checked:bg-emerald-600">
            </div>
            <span class="ms-3 text-sm font-medium text-slate-400">Manual</span>
        </label>
    </div>

    <!-- Visual Editor Container -->
    <div class="visual-editor-container space-y-6">
        <!-- Transport & Security -->
        <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
            <div>
                <label class="block text-sm font-medium text-slate-400 mb-2">Network (Transport)</label>
                <select
                    class="vis-network w-full bg-slate-900 border border-slate-700 rounded-lg px-4 py-2 text-slate-200 focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500 outline-none transition-colors">
                    <option value="tcp">TCP</option>
                    <option value="ws">WebSocket</option>
                    <option value="grpc">gRPC</option>
                    <option value="httpupgrade">HTTP Upgrade</option>
                    <option value="quic">QUIC</option>
                </select>
            </div>
            <div>
                <label class="block text-sm font-medium text-slate-400 mb-2">Security</label>
                <select
                    class="vis-security w-full bg-slate-900 border border-slate-700 rounded-lg px-4 py-2 text-slate-200 focus:border-emerald-500 focus:ring-1 focus:ring-emerald-500 outline-none transition-colors">
                    <option value="reality">Reality (XTLS)</option>
                    <option value="tls">TLS</option>
                    <option value="none">None</option>
                </select>
            </div>
        </div>

        <!-- Advanced Network Options -->
        <div class="bg-slate-800/30 p-4 rounded-lg border border-slate-700/50">
            <h4 class="text-xs font-bold text-slate-500 uppercase tracking-widest mb-3">Advanced Network Options</h4>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <!-- Sniffing Toggle -->
                <label class="flex items-center space-x-3 cursor-pointer group">
                    <input type="checkbox" class="vis-sniff sr-only peer">
                    <div class="w-9 h-5 bg-slate-700 peer-focus:ring-2 peer-focus:ring-emerald-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-emerald-600 relative"></div>
                    <span class="text-sm text-slate-400 group-hover:text-slate-200 transition-colors">Sniffing</span>
                </label>

                <!-- Sniff Override Dest -->
                <label class="vis-sniff-override-container hidden flex items-center space-x-3 cursor-pointer group">
                    <input type="checkbox" class="vis-sniff-override sr-only peer">
                    <div class="w-9 h-5 bg-slate-700 peer-focus:ring-2 peer-focus:ring-emerald-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-emerald-600 relative"></div>
                    <span class="text-sm text-slate-400 group-hover:text-slate-200 transition-colors">Override Dest</span>
                </label>

                <!-- Multiplex Toggle -->
                <label class="flex items-center space-x-3 cursor-pointer group">
                    <input type="checkbox" class="vis-multiplex sr-only peer">
                    <div class="w-9 h-5 bg-slate-700 peer-focus:ring-2 peer-focus:ring-emerald-800 rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-emerald-600 relative"></div>
                    <span class="text-sm text-slate-400 group-hover:text-slate-200 transition-colors">Multiplex</span>
                </label>
            </div>

            <!-- Routing Mode -->
            <div class="mt-4 pt-4 border-t border-slate-700/50">
                <label class="block text-xs font-bold text-slate-500 uppercase tracking-widest mb-2">Routing Mode</label>
                <div class="flex gap-4">
                    <label class="flex items-center space-x-2 cursor-pointer">
                        <input type="radio" name="vis_routing" value="smart" checked class="text-emerald-600 focus:ring-emerald-600 bg-slate-700 border-slate-600">
                        <span class="text-sm text-slate-300">Smart (Split-Tunneling)</span>
                    </label>
                    <label class="flex items-center space-x-2 cursor-pointer">
                        <input type="radio" name="vis_routing" value="global" class="text-emerald-600 focus:ring-emerald-600 bg-slate-700 border-slate-600">
                        <span class="text-sm text-slate-300">Global (All Traffic)</span>
                    </label>
                </div>
            </div>
        </div>

        <!-- Dynamic Sections -->

        <!-- Clients Section -->
        <div class="client-section bg-slate-800/50 p-4 rounded-lg border border-slate-700">
            <h4 class="text-sm font-semibold text-emerald-400 mb-3">Client Settings</h4>
            <div class="space-y-4">
                <div>
                    <label class="block text-xs text-slate-500 mb-1">UUID / Password</label>
                    <input type="text" value="{{uuid}}" disabled
                        class="w-full bg-slate-900/50 border border-slate-700 rounded px-3 py-1.5 text-slate-400 text-sm font-mono cursor-not-allowed">
                    <p class="text-xs text-slate-500 mt-1">Generated automatically per node.</p>
                </div>
                <div class="flow-section hidden">
                    <label class="block text-xs text-slate-500 mb-1">Flow</label>
                    <select
                        class="w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                        <option value="xtls-rprx-vision">xtls-rprx-vision</option>
                    </select>
                </div>
            </div>
        </div>

        <!-- Transport Settings -->
        <div class="vis-ws-settings transport-settings hidden bg-slate-800/50 p-4 rounded-lg border border-slate-700">
            <h4 class="text-sm font-semibold text-blue-400 mb-3">WebSocket Settings</h4>
            <div class="grid grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs text-slate-500 mb-1">Path</label>
                    <input type="text" value="/"
                        class="vis-ws-path w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                </div>
                <div>
                    <label class="block text-xs text-slate-500 mb-1">Host Header</label>
                    <input type="text" placeholder="{{sni}}"
                        class="vis-ws-host w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                </div>
            </div>
        </div>

        <div class="vis-grpc-settings transport-settings hidden bg-slate-800/50 p-4 rounded-lg border border-slate-700">
            <h4 class="text-sm font-semibold text-blue-400 mb-3">gRPC Settings</h4>
            <div>
                <label class="block text-xs text-slate-500 mb-1">Service Name</label>
                <input type="text" value="grpc"
                    class="vis-grpc-service w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
            </div>
        </div>

        <div
            class="vis-httpupgrade-settings transport-settings hidden bg-slate-800/50 p-4 rounded-lg border border-slate-700">
            <h4 class="text-sm font-semibold text-blue-400 mb-3">HTTP Upgrade Settings</h4>
            <div class="grid grid-cols-2 gap-4">
                <div>
                    <label class="block text-xs text-slate-500 mb-1">Path</label>
                    <input type="text" value="/"
                        class="vis-http-path w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                </div>
                <div>
                    <label class="block text-xs text-slate-500 mb-1">Host</label>
                    <input type="text" placeholder="{{sni}}"
                        class="vis-http-host w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                </div>
            </div>
        </div>

        <!-- Security Settings -->
        <div class="reality-settings hidden bg-slate-800/50 p-4 rounded-lg border border-slate-700">
            <h4 class="text-sm font-semibold text-purple-400 mb-3">Reality Settings</h4>
            <div class="space-y-4">
                <div class="grid grid-cols-2 gap-4">
                    <div>
                        <label class="block text-xs text-slate-500 mb-1">Dest (Target)</label>
                        <input type="text" placeholder="www.google.com:443"
                            class="vis-reality-dest w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                    </div>
                    <div>
                        <label class="block text-xs text-slate-500 mb-1">Server Names (SNI)</label>
                        <input type="text" placeholder="www.google.com,gateway.icloud.com"
                            class="vis-reality-sni w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
                    </div>
                </div>
                <div>
                    <label class="block text-xs text-slate-500 mb-1">Private Key</label>
                    <div class="flex gap-2">
                        <input type="text" value="{{reality_private}}" disabled
                            class="w-full bg-slate-900/50 border border-slate-700 rounded px-3 py-1.5 text-slate-400 text-sm font-mono cursor-not-allowed">
                        <button type="button" onclick="alert('Keys generated by server')"
                            class="px-3 py-1.5 bg-slate-700 hover:bg-slate-600 text-slate-200 rounded text-xs transition-colors">
                            Regenerate
                        </button>
                    </div>
                </div>
                <div>
                    <label class="block text-xs text-slate-500 mb-1">Short IDs</label>
                    <input type="text" value='["", "0123456789abcdef"]' disabled
                        class="w-full bg-slate-900/50 border border-slate-700 rounded px-3 py-1.5 text-slate-400 text-sm font-mono cursor-not-allowed">
                </div>
            </div>
        </div>

        <div class="tls-settings hidden bg-slate-800/50 p-4 rounded-lg border border-slate-700">
            <h4 class="text-sm font-semibold text-purple-400 mb-3">TLS Settings</h4>
            <div>
                <label class="block text-xs text-slate-500 mb-1">Server Name (SNI)</label>
                <input type="text" placeholder="{{sni}}"
                    class="vis-tls-sni w-full bg-slate-900 border border-slate-700 rounded px-3 py-1.5 text-slate-200 text-sm">
            </div>
        </div>
    </div>

    <!-- Manual JSON Editor (Hidden by Default) -->
    <div class="manual-editor-container hidden space-y-4">
        <div class="p-4 bg-yellow-500/10 border border-yellow-500/20 rounded-lg mb-4">
            <p class="text-sm text-yellow-400">
                <i data-lucide="alert-triangle" class="w-4 h-4 inline mr-1"></i>
                Manual Mode: Edits here will override Visual settings. Switch back to Visual to attempt parsing.
            </p>
        </div>

        <div>
            <label class="block text-sm font-medium text-slate-400 mb-2">Settings JSON</label>
            <div class="relative group overflow-hidden rounded-2xl border border-white/5 bg-[#0d1117]">
                <div class="absolute top-0 left-0 right-0 h-1 bg-indigo-500/30"></div>
                <!-- Note: using name="settings_template" which matches both Create and Edit forms -->
                <textarea name="settings_template" rows="10" required
                    class="w-full bg-transparent p-4 text-xs text-indigo-300 font-mono focus:outline-none resize-none leading-relaxed transition-all"
                    placeholder='{"protocol": "vless", "clients": []}'></textarea>
            </div>
        </div>

        <div>
            <label class="block text-sm font-medium text-slate-400 mb-2">Stream Settings JSON</label>
            <div class="relative group overflow-hidden rounded-2xl border border-white/5 bg-[#0d1117]">
                <div class="absolute top-0 left-0 right-0 h-1 bg-purple-500/30"></div>
                <textarea name="stream_settings_template" rows="10" required
                    class="w-full bg-transparent p-4 text-xs text-purple-300 font-mono focus:outline-none resize-none leading-relaxed transition-all"
                    placeholder='{"network": "tcp", ...}'></textarea>
            </div>
        </div>
    </div>
</div>
            </div>
        </form>

        <script src="admin_templates_editor.js"></script>
        <script>
            // Initialize the editor
            document.addEventListener('DOMContentLoaded', () => {
                const form = document.getElementById('tplForm');
                if (form) {
                    new VisualEditor(form.querySelector('#editor-container'));
                    lucide.createIcons();
                }
            });
        </script>
    </body>
    </html>
    """
    with open("frontend_verification/verify.html", "w") as f:
        f.write(html_content)

def verify_ui():
    generate_mock_html()

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page()

        # Load local HTML file
        file_path = os.path.abspath("frontend_verification/verify.html")
        page.goto(f"file://{file_path}")

        # 1. Verify Advanced Options
        page.wait_for_selector(".vis-sniff")
        page.wait_for_selector(".vis-multiplex")

        # 2. Verify Routing Mode
        page.wait_for_selector("input[name='vis_routing']")

        # Interact with Sniffing to show override
        page.click("label:has(.vis-sniff)")
        page.wait_for_selector(".vis-sniff-override-container:not(.hidden)", timeout=5000)

        # Check that routing mode changes generate JSON (implicitly verified by functionality, here just checking presence)

        # Take final screenshot
        page.screenshot(path="frontend_verification/ui_verification_final.png", full_page=True)

        browser.close()

if __name__ == "__main__":
    verify_ui()

// Browser entry point for the debugger's preview pipeline.
//
// The pipeline is the bridge's, unchanged: `renderPreview` from
// ../plugin-renderer.cjs drives the extension's own `out/*.js` modules, and
// this file only swaps the host (wasm modules instead of subprocesses). Keeping
// one implementation is what makes the static page and the bridge agree.

import * as dot from "@plugin/dot.js";
import * as mathView from "@plugin/mathView.js";
import * as ir from "@plugin/ir.js";
import * as eggPreviewSource from "@plugin/eggPreviewSource.js";
import * as eggVizAnnotation from "@plugin/eggVizAnnotation.js";
import * as eggRuleMapping from "@plugin/eggRuleMapping.js";
import * as rustBindingRename from "@plugin/rustBindingRename.js";
import * as typstCore from "@plugin/shared/typstCore.js";
import { renderPreview, validateTemplate } from "../../plugin-renderer.cjs";
import { createBrowserHost } from "./host.js";
import { previewRequest } from "./annotations.js";

const REVISION = typeof __EGGLOG_BROWSER_REVISION__ === "string" ? __EGGLOG_BROWSER_REVISION__ : "browser-wasm";

const MODULES = {
    dot,
    mathView,
    ir,
    eggPreviewSource,
    eggVizAnnotation,
    eggRuleMapping,
    rustBindingRename,
    "shared/typstCore": typstCore
};

export function createRenderer(options = {}) {
    const host = createBrowserHost({
        assetBase: options.assetBase || new URL("./assets/", import.meta.url),
        modules: MODULES,
        revision: options.revision || REVISION
    });
    return {
        revision: host.revision,
        // The bridge's `/api/preview` adds the rule entry and the birewrite
        // alias in Python before rendering; the page does it here.
        renderPreview: request => renderPreview(host, { ...previewRequest(request.source, request.line), ...request }),
        validateTemplate: (template, fields) => validateTemplate(host, template, fields)
    };
}

let shared = null;
export function renderer() {
    if (!shared) shared = createRenderer();
    return shared;
}

export const renderPreviewInBrowser = request => renderer().renderPreview(request);
export const validateTemplateInBrowser = (template, fields) => renderer().validateTemplate(template, fields);

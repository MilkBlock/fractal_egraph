// Browser entry point for the debugger's preview pipeline.
//
// The pipeline is the bridge's, unchanged: `renderPreview` from
// ../../plugin-renderer.cjs drives the extension's own `out/*.js` modules, and
// this file only swaps the host (wasm modules instead of subprocesses). Keeping
// one implementation is what makes the static page and the bridge agree.
//
// The two annotations the bridge's `/api/preview` and `/api/edit-*` endpoints
// get from egglog-demo's `preview_annotations.py` are answered by
// ./annotations.js instead.

import * as dot from "@plugin/dot.js";
import * as mathView from "@plugin/mathView.js";
import * as ir from "@plugin/ir.js";
import * as eggPreviewSource from "@plugin/eggPreviewSource.js";
import * as eggVizAnnotation from "@plugin/eggVizAnnotation.js";
import * as eggRuleMapping from "@plugin/eggRuleMapping.js";
import * as rustBindingRename from "@plugin/rustBindingRename.js";
import * as sharedTypstCore from "@plugin/shared/typstCore.js";
import { renderPreview, validateTemplate } from "../../plugin-renderer.cjs";
import { createBrowserHost } from "./host.js";
import { catalog, previewRequest, updateConditions, updateDisplay } from "./annotations.js";

const REVISION = typeof __EGGLOG_BROWSER_REVISION__ === "string" ? __EGGLOG_BROWSER_REVISION__ : "browser-wasm";

const MODULES = {
    dot,
    mathView,
    ir,
    eggPreviewSource,
    eggVizAnnotation,
    eggRuleMapping,
    rustBindingRename,
    "shared/typstCore": sharedTypstCore
};

function failure(message, payload) {
    const error = new Error(message);
    if (payload) error.payload = payload;
    return error;
}

export function createRenderer(options = {}) {
    const host = createBrowserHost({
        assetBase: options.assetBase || new URL("./assets/", import.meta.url),
        modules: MODULES,
        revision: options.revision || REVISION
    });
    return {
        revision: host.revision,
        // `/api/preview` adds the rule entry, the birewrite alias and the
        // editable targets before rendering; the page does it here.
        renderPreview: request => renderPreview(host, {
            ...previewRequest(request.source, request.line),
            edit_targets: catalog(request.source, request.line),
            ...request
        }),
        validateTemplate: (template, fields) => validateTemplate(host, template, fields),
        // Same contract as the bridge: a template that does not compile is
        // refused, and a repairable one comes back as a suggestion the user has
        // to confirm, never as a silent write.
        async editDisplay(body) {
            const result = updateDisplay(body.source, body.line, body.target_id, body.value,
                body.fields ?? null, body.template ?? null, body.precedence ?? null);
            if (result.template) {
                const check = await validateTemplate(host, result.template, result.template_fields || []);
                if (!check.ok) {
                    const message = 'Typst 模板无法编译：' + (check.error || '未知错误');
                    if (check.suggestion && check.suggestion !== result.template) {
                        throw failure(message, { error: message, suggestion: check.suggestion, notes: check.notes || [] });
                    }
                    throw failure(message, { error: message });
                }
            }
            return result;
        },
        editConditions: body => updateConditions(body.source, body.line, body.conditions)
    };
}

let shared = null;
export function renderer() {
    if (!shared) shared = createRenderer();
    return shared;
}

export const renderPreviewInBrowser = request => renderer().renderPreview(request);
export const validateTemplateInBrowser = (template, fields) => renderer().validateTemplate(template, fields);
export const editDisplayInBrowser = body => renderer().editDisplay(body);
export const editConditionsInBrowser = body => renderer().editConditions(body);

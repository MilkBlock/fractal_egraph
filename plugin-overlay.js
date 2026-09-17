const parseSvgDimension = (svgMarkup, attr) => {
        const match = svgMarkup.match(new RegExp(attr + '="([0-9.]+)(?:pt)?"'));
        return match ? Number(match[1]) : 0;
      };
const encodeSvgDataUri = (svgMarkup) => {
        return "data:image/svg+xml;base64," + btoa(unescape(encodeURIComponent(svgMarkup)));
      };
const findNodeShape = (nodeGroup) => {
        return Array.from(nodeGroup.children).find((child) => {
          return child.tagName !== "title" && child.tagName !== "text" && child.tagName !== "image";
        });
      };
const applyTypstRenderings = (root, typstRenderings) => {
        const nodeGroups = Array.from(root.querySelectorAll("g.node"));
        for (const [targetId, rendered] of Object.entries(typstRenderings || {})) {
          const nodeGroup = nodeGroups.find((group) => group.querySelector("title")?.textContent === targetId);
          if (!nodeGroup) {
            continue;
          }

          const shape = findNodeShape(nodeGroup);
          if (!shape) {
            continue;
          }

          const textNodes = Array.from(nodeGroup.querySelectorAll("text"));
          const textLines = textNodes
            .map((node) => node.textContent?.trim() || "")
            .filter((line) => line.length > 0);
          const annotationLines = textLines.slice(1);

          const bbox = shape.getBBox();
          const formulaWidth = rendered.width || parseSvgDimension(rendered.svg, "width");
          const formulaHeight = rendered.height || parseSvgDimension(rendered.svg, "height");
          if (!formulaWidth || !formulaHeight) {
            // fallback: keep the original text label untouched
            continue;
          }

          const annotationLineHeight = 12;
          const annotationGap = annotationLines.length > 0 ? 4 : 0;
          const annotationBlockHeight = annotationLines.length * annotationLineHeight + annotationGap;
          const maxWidth = bbox.width * 0.92;
          const maxHeight = bbox.height * 0.86 - annotationBlockHeight;
          if (maxWidth <= 0 || maxHeight <= 0) {
            // fallback: insufficient room for overlay, keep plain text label
            continue;
          }

          const scale = Math.min(maxWidth / formulaWidth, maxHeight / formulaHeight);
          if (!Number.isFinite(scale) || scale <= 0) {
            continue;
          }

          const width = formulaWidth * scale;
          const height = formulaHeight * scale;
          const contentTop = bbox.y + (bbox.height - (height + annotationBlockHeight)) / 2;
          const image = document.createElementNS("http://www.w3.org/2000/svg", "image");
          image.setAttribute("href", encodeSvgDataUri(rendered.svg));
          image.setAttribute("x", String(bbox.x + (bbox.width - width) / 2));
          image.setAttribute("y", String(contentTop));
          image.setAttribute("width", String(width));
          image.setAttribute("height", String(height));
          image.setAttribute("data-typst-rendering", "true");
          image.setAttribute("pointer-events", "none");

          const annotationOverlay = document.createElementNS("http://www.w3.org/2000/svg", "g");
          annotationOverlay.setAttribute("data-typst-annotation-overlay", "true");
          annotationOverlay.setAttribute("pointer-events", "none");
          for (let index = 0; index < annotationLines.length; index += 1) {
            const line = annotationLines[index];
            const text = document.createElementNS("http://www.w3.org/2000/svg", "text");
            text.setAttribute("x", String(bbox.x + bbox.width / 2));
            text.setAttribute("y", String(contentTop + height + annotationGap + (index + 1) * annotationLineHeight - 2));
            text.setAttribute("text-anchor", "middle");
            text.setAttribute("font-size", "10");
            text.setAttribute("fill", "var(--vscode-editor-foreground)");
            text.setAttribute("stroke", "var(--vscode-editor-background)");
            text.setAttribute("stroke-width", "0.8");
            text.setAttribute("paint-order", "stroke");
            text.textContent = line;
            annotationOverlay.appendChild(text);
          }

          // Typst overlay succeeded: replace label text with image+annotation overlay.
          for (const textNode of textNodes) {
            textNode.remove();
          }
          nodeGroup.appendChild(image);
          if (annotationLines.length > 0) {
            nodeGroup.appendChild(annotationOverlay);
          }
        }
      };
export { applyTypstRenderings };

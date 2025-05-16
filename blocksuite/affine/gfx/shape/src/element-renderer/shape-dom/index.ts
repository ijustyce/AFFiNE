import type { DomRenderer } from '@blocksuite/affine-block-surface';
import type { ShapeElementModel } from '@blocksuite/affine-model';
import { DefaultTheme } from '@blocksuite/affine-model';

import { manageClassNames, setStyles } from './utils';

function applyShapeSpecificStyles(
  model: ShapeElementModel,
  element: HTMLElement,
  zoom: number
) {
  // Reset properties that might be set by different shape types
  element.style.clipPath = '';
  element.style.borderRadius = '';
  // Clear innerHTML for shapes that don't use SVG, or if type changes from SVG-based to non-SVG-based
  if (model.shapeType !== 'diamond' && model.shapeType !== 'triangle') {
    element.innerHTML = '';
  }

  if (model.shapeType === 'rect') {
    const w = model.w * zoom;
    const h = model.h * zoom;
    const r = model.radius ?? 0;
    const borderRadius =
      r < 1 ? `${Math.min(w * r, h * r)}px` : `${r * zoom}px`;
    element.style.borderRadius = borderRadius;
  } else if (model.shapeType === 'ellipse') {
    element.style.borderRadius = '50%';
  } else if (model.shapeType === 'diamond') {
    element.style.clipPath = 'polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%)';
  } else if (model.shapeType === 'triangle') {
    element.style.clipPath = 'polygon(50% 0%, 100% 100%, 0% 100%)';
  }
  // No 'else' needed to clear styles, as they are reset at the beginning of the function.
}

function applyBorderStyles(
  model: ShapeElementModel,
  element: HTMLElement,
  strokeColor: string,
  zoom: number
) {
  element.style.border =
    model.strokeStyle !== 'none'
      ? `${model.strokeWidth * zoom}px ${model.strokeStyle === 'dash' ? 'dashed' : 'solid'} ${strokeColor}`
      : 'none';
}

function applyTransformStyles(model: ShapeElementModel, element: HTMLElement) {
  if (model.rotate && model.rotate !== 0) {
    setStyles(element, {
      transform: `rotate(${model.rotate}deg)`,
      transformOrigin: 'center',
    });
  } else {
    setStyles(element, {
      transform: '',
      transformOrigin: '',
    });
  }
}

function applyShadowStyles(
  model: ShapeElementModel,
  element: HTMLElement,
  renderer: DomRenderer
) {
  if (model.shadow) {
    const { offsetX, offsetY, blur, color } = model.shadow;
    setStyles(element, {
      boxShadow: `${offsetX}px ${offsetY}px ${blur}px ${renderer.getColorValue(color)}`,
    });
  } else {
    setStyles(element, { boxShadow: '' });
  }
}

/**
 * Renders a ShapeElementModel to a given HTMLElement using DOM properties.
 * This function is intended to be registered via the DomElementRendererExtension.
 *
 * @param model - The shape element model containing rendering properties.
 * @param element - The HTMLElement to apply the shape's styles to.
 * @param renderer - The main DOMRenderer instance, providing access to viewport and color utilities.
 */
export const shapeDomRenderer = (
  model: ShapeElementModel,
  element: HTMLElement,
  renderer: DomRenderer
): void => {
  const { zoom } = renderer.viewport;
  const unscaledWidth = model.w;
  const unscaledHeight = model.h;

  const fillColor = renderer.getColorValue(
    model.fillColor,
    DefaultTheme.shapeFillColor,
    true
  );
  const strokeColor = renderer.getColorValue(
    model.strokeColor,
    DefaultTheme.shapeStrokeColor,
    true
  );

  element.style.width = `${unscaledWidth * zoom}px`;
  element.style.height = `${unscaledHeight * zoom}px`;
  element.style.boxSizing = 'border-box';

  // Apply shape-specific clipping, border-radius, and potentially clear innerHTML
  applyShapeSpecificStyles(model, element, zoom);

  if (model.shapeType === 'diamond' || model.shapeType === 'triangle') {
    // For diamond and triangle, fill and border are handled by inline SVG
    element.style.border = 'none'; // Ensure no standard CSS border interferes
    element.style.backgroundColor = 'transparent'; // Host element is transparent

    const strokeW = model.strokeWidth;
    const halfStroke = strokeW / 2; // Calculate half stroke width for point adjustment

    let svgPoints = '';
    if (model.shapeType === 'diamond') {
      // Adjusted points for diamond
      svgPoints = `\
${unscaledWidth / 2},${halfStroke} \
${unscaledWidth - halfStroke},${unscaledHeight / 2} \
${unscaledWidth / 2},${unscaledHeight - halfStroke} \
${halfStroke},${unscaledHeight / 2}`;
    } else {
      // triangle
      // Adjusted points for triangle
      svgPoints = `\
${unscaledWidth / 2},${halfStroke} \
${unscaledWidth - halfStroke},${unscaledHeight - halfStroke} \
${halfStroke},${unscaledHeight - halfStroke}`;
    }

    // Determine if stroke should be visible and its color
    const finalStrokeColor =
      model.strokeStyle !== 'none' && strokeW > 0 ? strokeColor : 'transparent';
    // Determine dash array, only if stroke is visible and style is 'dash'
    const finalStrokeDasharray =
      model.strokeStyle === 'dash' && finalStrokeColor !== 'transparent'
        ? '12, 12'
        : 'none';
    // Determine fill color
    const finalFillColor = model.filled ? fillColor : 'transparent';

    element.innerHTML = `
      <svg width="100%" height="100%" viewBox="0 0 ${unscaledWidth} ${unscaledHeight}" preserveAspectRatio="none">
        <polygon
          points="${svgPoints}"
          fill="${finalFillColor}"
          stroke="${finalStrokeColor}"
          stroke-width="${strokeW}"
          stroke-dasharray="${finalStrokeDasharray}"
        />
      </svg>
    `;
  } else {
    // Standard rendering for other shapes (e.g., rect, ellipse)
    // innerHTML was already cleared by applyShapeSpecificStyles if necessary
    element.style.backgroundColor = model.filled ? fillColor : 'transparent';
    applyBorderStyles(model, element, strokeColor, zoom); // Uses standard CSS border
  }

  applyTransformStyles(model, element);

  element.style.zIndex = renderer.layerManager.getZIndex(model).toString();

  manageClassNames(model, element);
  applyShadowStyles(model, element, renderer);
};

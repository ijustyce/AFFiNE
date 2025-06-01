import {
  AffineCanvasTextFonts,
  FontConfigExtension,
} from '@blocksuite/affine/shared/services';

export function getFontConfigExtension() {
  return FontConfigExtension(
    AffineCanvasTextFonts.map(font => ({
      ...font,
      url: 'https://cdn.kpromise.top/affine/fonts/' + font.url.split('/').pop(),
    }))
  );
}

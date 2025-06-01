import {
  AffineCanvasTextFonts,
  FontConfigExtension,
} from '@blocksuite/affine/shared/services';

export function getFontConfigExtension() {
  const cdnUrl = process.env.AFFINE_CDN_HOST ?? environment.publicPath;
  return FontConfigExtension(
    AffineCanvasTextFonts.map(font => ({
      ...font,
      url: 'https://cdn.kpromise.top/affine/fonts/' + font.url.split('/').pop(),
    }))
  );
}

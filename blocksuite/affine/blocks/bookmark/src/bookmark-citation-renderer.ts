import { BookmarkBlockSchema } from '@blocksuite/affine-model';
import { CitationRendererExtension } from '@blocksuite/affine-shared/services';
import { html } from 'lit';
import { classMap } from 'lit/directives/class-map.js';
import { when } from 'lit/directives/when.js';

import { type BookmarkBlockComponent } from './bookmark-block';

export const BookmarkCitationRendererExtension = CitationRendererExtension(
  BookmarkBlockSchema.model.flavour,
  (block: BookmarkBlockComponent, content: unknown) => {
    return when(
      block.isCitation,
      () => {
        const {
          citationService,
          blockDraggable,
          selectedStyle$,
          selected$,
          containerStyleMap,
          handleClick,
          handleDoubleClick,
          model,
        } = block;
        const { url, footnoteIdentifier } = model.props;
        const { icon, title, description } = block.linkPreview$.value;
        const iconSrc = icon
          ? block.imageProxyService.buildUrl(icon)
          : undefined;

        return html`
          <div
            draggable="${blockDraggable ? 'true' : 'false'}"
            class=${classMap({
              'affine-bookmark-container': true,
              ...selectedStyle$?.value,
            })}
            style=${containerStyleMap}
          >
            ${citationService.renderCard({
              icon: iconSrc,
              title: title || url,
              content: description ?? undefined,
              identifier: footnoteIdentifier ?? '',
              onClickCallback: handleClick,
              onDoubleClickCallback: handleDoubleClick,
              active: selected$.value,
            })}
          </div>
        `;
      },
      () => content
    );
  }
);

import { EmbedLinkedDocBlockSchema } from '@blocksuite/affine-model';
import { CitationRendererExtension } from '@blocksuite/affine-shared/services';
import { html } from 'lit';
import { classMap } from 'lit/directives/class-map.js';
import { styleMap } from 'lit/directives/style-map.js';
import { when } from 'lit/directives/when.js';

import type { EmbedLinkedDocBlockComponent } from './embed-linked-doc-block';

export const EmbedLinkedDocCitationRendererExtension =
  CitationRendererExtension(
    EmbedLinkedDocBlockSchema.model.flavour,
    (block: EmbedLinkedDocBlockComponent, content: unknown) => {
      return when(
        block.isCitation,
        () => {
          const {
            citationService,
            model,
            icon$,
            title$,
            selected$,
            handleClick,
            handleDoubleClick,
            blockDraggable,
            selectedStyle$,
            embedContainerStyle,
          } = block;
          const { footnoteIdentifier } = model.props;

          return html`<div
            draggable="${blockDraggable ? 'true' : 'false'}"
            class=${classMap({
              'embed-block-container': true,
              ...selectedStyle$?.value,
            })}
            style=${styleMap({
              ...embedContainerStyle,
            })}
          >
            ${citationService.renderCard({
              icon: icon$.value,
              title: title$.value.value,
              identifier: footnoteIdentifier ?? '',
              active: selected$.value,
              onClickCallback: handleClick,
              onDoubleClickCallback: handleDoubleClick,
            })}
          </div> `;
        },
        () => content
      );
    }
  );

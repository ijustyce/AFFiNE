import { getAttachmentFileIcon } from '@blocksuite/affine-components/icons';
import { AttachmentBlockSchema } from '@blocksuite/affine-model';
import { CitationRendererExtension } from '@blocksuite/affine-shared/services';
import { html } from 'lit';
import { classMap } from 'lit/directives/class-map.js';
import { when } from 'lit/directives/when.js';

import type { AttachmentBlockComponent } from './attachment-block';

export const AttachmentCitationRendererExtension = CitationRendererExtension(
  AttachmentBlockSchema.model.flavour,
  (block: AttachmentBlockComponent, content: unknown) => {
    return when(
      block.isCitation,
      () => {
        const {
          citationService,
          selected$,
          containerStyleMap,
          onClick,
          model,
          filetype,
        } = block;
        const { name, footnoteIdentifier } = model.props;
        const fileTypeIcon = getAttachmentFileIcon(filetype);

        return html`
          <div
            class=${classMap({
              'affine-attachment-container': true,
              focused: selected$.value,
            })}
            style=${containerStyleMap}
          >
            ${citationService.renderCard({
              icon: fileTypeIcon,
              title: name,
              identifier: footnoteIdentifier ?? '',
              active: selected$.value,
              onClickCallback: onClick,
            })}
          </div>
        `;
      },
      () => content
    );
  }
);

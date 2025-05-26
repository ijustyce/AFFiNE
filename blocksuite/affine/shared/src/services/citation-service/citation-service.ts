import { type Container, createIdentifier } from '@blocksuite/global/di';
import { type BlockStdScope, StdIdentifier } from '@blocksuite/std';
import { type BlockModel, Extension } from '@blocksuite/store';
import { html, type TemplateResult } from 'lit';

import { DocModeProvider } from '../doc-mode-service';
import type {
  CitationEvents,
  CitationEventType,
} from '../telemetry-service/citation';
import { TelemetryProvider } from '../telemetry-service/telemetry-service';

const CitationEventTypeMap = {
  Hover: 'AICitationHoverSource',
  Expand: 'AICitationExpandSource',
  Delete: 'AICitationDelete',
  Edit: 'AICitationEdit',
} as const;

type EventType = keyof typeof CitationEventTypeMap;

type EventTypeMapping = {
  [K in EventType]: CitationEventType;
};

export interface CitationCardOptions {
  icon?: TemplateResult | string;
  title: string;
  content?: string;
  identifier: string;
  onClickCallback?: (e: MouseEvent) => void;
  onDoubleClickCallback?: (e: MouseEvent) => void;
  active?: boolean;
}

export interface CitationViewService {
  /**
   * Renders a citation card with the provided options
   * @param options - The options for the citation card
   * @returns A template result or string
   */
  renderCard(options: CitationCardOptions): TemplateResult | string;
  /**
   * Tracks citation-related events
   * @param type - The type of citation event to track
   * @param properties - The properties of the event
   */
  trackEvent<T extends EventType>(
    type: T,
    properties?: CitationEvents[EventTypeMapping[T]]
  ): void;
  /**
   * Checks if the model is a citation model
   * @param model - The model to check
   * @returns True if the model is a citation model, false otherwise
   */
  isCitationModel(model: BlockModel): boolean;
}

export const CitationProvider =
  createIdentifier<CitationService>('CitationService');

export class CitationService extends Extension implements CitationViewService {
  constructor(
    private readonly std: BlockStdScope,
    private readonly docModeService: DocModeProvider
  ) {
    super();
  }

  static override setup(di: Container) {
    di.addImpl(CitationProvider, CitationService, [
      StdIdentifier,
      DocModeProvider,
    ]);
  }

  get telemetryService() {
    return this.std.getOptional(TelemetryProvider);
  }

  isCitationModel = (model: BlockModel) => {
    return (
      'footnoteIdentifier' in model.props &&
      !!model.props.footnoteIdentifier &&
      'style' in model.props &&
      model.props.style === 'citation'
    );
  };

  renderCard(options: CitationCardOptions) {
    return html`<affine-citation-card
      .icon=${options.icon}
      .citationTitle=${options.title}
      .citationContent=${options.content}
      .citationIdentifier=${options.identifier}
      .onClickCallback=${options.onClickCallback}
      .onDoubleClickCallback=${options.onDoubleClickCallback}
      .active=${!!options.active}
    ></affine-citation-card>`;
  }

  trackEvent<T extends EventType>(
    type: T,
    properties?: CitationEvents[EventTypeMapping[T]]
  ) {
    const editorMode = this.docModeService.getEditorMode() ?? 'page';
    this.telemetryService?.track(CitationEventTypeMap[type], {
      page: editorMode === 'page' ? 'doc editor' : 'whiteboard editor',
      module: 'AI Result',
      control: 'Source',
      ...properties,
    });
  }
}

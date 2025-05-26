import { DisposableGroup } from '@blocksuite/global/disposable';
import { type BlockComponent, LifeCycleWatcher } from '@blocksuite/std';
import type { ExtensionType } from '@blocksuite/store';
import { filter } from 'rxjs/operators';

type CitationRenderer<T extends BlockComponent> = (
  block: T,
  content: unknown
) => unknown;

/**
 * Creates an extension for registering a CitationRenderer for a specific block flavour.
 *
 * @param flavour The flavour of the block that the renderer is for
 * @param renderer The renderer function that handles rendering for this block type
 * @returns An ExtensionType object that can be used to set up the renderer in the DI container
 */
export const CitationRendererExtension = <T extends BlockComponent>(
  flavour: string,
  renderer: CitationRenderer<T>
): ExtensionType => {
  const name = flavour.split(':').pop();
  return class CitationRenderer extends LifeCycleWatcher {
    static override key = `${name}-citation-renderer-extension`;

    private readonly _disposables = new DisposableGroup();

    override mounted() {
      const subscription = this.std.view.viewUpdated
        .pipe(
          filter(payload => {
            const { type, method, view } = payload;
            return (
              type === 'block' &&
              view.model.flavour === flavour &&
              method === 'add'
            );
          })
        )
        .subscribe(({ view }) => {
          const blockView = view as T;
          blockView.addRenderer(content => renderer(blockView, content));
        });
      this._disposables.add(subscription);
    }

    override unmounted() {
      this._disposables.dispose();
    }
  };
};

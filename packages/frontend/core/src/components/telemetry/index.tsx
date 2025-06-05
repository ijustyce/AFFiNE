import { mixpanel, sentry } from '@affine/track';
import { appSettingAtom } from '@toeverything/infra';
import { useAtomValue } from 'jotai/react';
import { useEffect } from 'react';

export function Telemetry() {
  const settings = useAtomValue(appSettingAtom);

  useEffect(() => {
    sentry.disable();
    mixpanel.opt_out_tracking();
    return;
  }, [settings.enableTelemetry]);

  return null;
}

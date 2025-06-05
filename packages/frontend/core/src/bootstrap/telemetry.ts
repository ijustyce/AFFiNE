import { mixpanel, sentry } from '@affine/track';

sentry.disable();
mixpanel.opt_out_tracking();

import { mixpanel, sentry } from '@affine/track';

mixpanel.opt_out_tracking();
sentry.disable();

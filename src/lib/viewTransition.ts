import { flushSync } from "react-dom";

type ViewTransitionDocument = Document & {
  startViewTransition?: (callback: () => void) => unknown;
};

export function runViewTransition(update: () => void) {
  const startViewTransition = (document as ViewTransitionDocument).startViewTransition;
  if (!startViewTransition) {
    update();
    return;
  }

  let didUpdate = false;
  try {
    startViewTransition.call(document, () => {
      didUpdate = true;
      flushSync(update);
    });
  } catch (error) {
    if (!didUpdate) update();
    else throw error;
  }
}

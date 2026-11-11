// In-app guidance for the agent console: a first-visit guided tour (spotlight + tooltip) and a
// scroll-along guide that explains each step of a purchase as it reaches the middle of the screen.
// Plain DOM only; no network calls. Every text comes from this file or from server-rendered,
// escaped data-guide attributes. Survives the console's live reloads via sessionStorage.
(() => {
  'use strict';
  const store = {
    get(area, k) { try { return window[area].getItem(k); } catch { return null; } },
    set(area, k, v) { try { window[area].setItem(k, v); } catch { /* storage may be blocked */ } },
    del(area, k) { try { window[area].removeItem(k); } catch { /* ignore */ } },
  };
  const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };

  // ---------- Guided tour ----------
  const STEPS = [
    { sel: '#agent-request', title: 'Say what you want',
      text: 'Type a request in plain English with a budget, for example “Buy me an LED desk lamp under $60 with tracked delivery.” The amount you write is a hard cap the AI cannot raise.' },
    { sel: 'label.external-discovery', title: 'Let it search real products',
      text: 'Searches the connected Shopify store first, then Channel3. Only the search terms leave QuarantinePay, never your budget, address or request.' },
    { sel: 'label.use-mandate', title: 'Buy without clicking, within limits',
      text: 'With an active mandate the agent buys on its own: PayPal pays from your saved Sandbox wallet, but only inside the cap and budget you signed.' },
    { sel: '.request-box button.primary-lg', title: 'Start the agent',
      text: 'Planning and screening take about 20–70 seconds on the local AI model. The page updates by itself; scroll down to follow each step.' },
    { sel: 'section.mandate-card', title: 'Your spending mandate',
      text: 'Per-purchase cap, total budget and expiry. Revoke it at any time and the agent immediately loses its authority to buy.' },
    { sel: 'section.task-list', title: 'Every purchase and its outcome',
      text: 'Captured, voided, held for review. Open any task to see its full pipeline: discovery, contract, PayPal hold, delivery checks and settlement.' },
    { sel: 'ol.pipeline', title: 'Scroll through a purchase',
      text: 'Each numbered step is one stage of the purchase. As you scroll, the guide in the corner explains what happens there and what to look at.' },
    { sel: 'a.nav[href="/intelligence"]', title: 'All payments in one grid',
      text: 'Intelligence lists every hold, capture and void with its policy flags (AG Grid).' },
  ];
  const TOUR_STEP = 'qp-tour-step';
  const TOUR_DONE = 'qp-tour-done';
  let tour = null;

  function visibleSteps() {
    return STEPS.filter((s) => {
      const t = document.querySelector(s.sel);
      return t && t.getClientRects().length > 0;
    });
  }

  function endTour(done) {
    if (!tour) return;
    tour.root.remove();
    document.body.classList.remove('touring');
    window.removeEventListener('resize', tour.place);
    window.removeEventListener('scroll', tour.place, true);
    document.removeEventListener('keydown', tour.keys, true);
    if (tour.opener && document.contains(tour.opener)) tour.opener.focus();
    tour = null;
    store.del('sessionStorage', TOUR_STEP);
    if (done) store.set('localStorage', TOUR_DONE, '1');
  }

  function startTour(from, opener) {
    endTour(false);
    const steps = visibleSteps();
    if (!steps.length) return;
    const root = el('div', 'tour');
    root.setAttribute('role', 'dialog');
    root.setAttribute('aria-modal', 'true');
    root.setAttribute('aria-labelledby', 'tour-title');
    const spot = el('div', 'tour-spot');
    const tip = el('div', 'tour-tip');
    const count = el('div', 'tour-count');
    const title = el('h2', 'tour-title');
    title.id = 'tour-title';
    const text = el('p', 'tour-text');
    const dots = el('div', 'tour-dots');
    const nav = el('div', 'tour-nav');
    const skip = el('button', 'tour-skip', 'Skip tour');
    const back = el('button', 'tour-back', 'Back');
    const next = el('button', 'tour-next', 'Next');
    skip.type = back.type = next.type = 'button';
    nav.append(skip, el('span', 'tour-gap'), back, next);
    tip.append(count, title, text, dots, nav);
    root.append(spot, tip);
    document.body.append(root);
    document.body.classList.add('touring');

    tour = { root, steps, i: Math.min(Math.max(from, 0), steps.length - 1), opener };
    const show = () => {
      const s = tour.steps[tour.i];
      store.set('sessionStorage', TOUR_STEP, String(tour.i));
      count.textContent = `Step ${tour.i + 1} of ${tour.steps.length}`;
      title.textContent = s.title;
      text.textContent = s.text;
      dots.replaceChildren(...tour.steps.map((_, k) => el('span', k === tour.i ? 'on' : '')));
      back.disabled = tour.i === 0;
      next.textContent = tour.i === tour.steps.length - 1 ? 'Finish' : 'Next';
      const target = document.querySelector(s.sel);
      target.scrollIntoView({ block: 'center', behavior: reduced() ? 'auto' : 'smooth' });
      tour.place();
      window.setTimeout(tour.place, reduced() ? 0 : 450);
      next.focus({ preventScroll: true });
    };
    tour.place = () => {
      if (!tour) return;
      const target = document.querySelector(tour.steps[tour.i].sel);
      if (!target) return;
      const r = target.getBoundingClientRect();
      const pad = 8;
      Object.assign(spot.style, {
        top: `${r.top - pad}px`, left: `${r.left - pad}px`,
        width: `${r.width + pad * 2}px`, height: `${r.height + pad * 2}px`,
      });
      if (window.innerWidth < 700) { // phone: tooltip docks to the bottom edge
        tip.classList.add('docked');
        tip.style.top = tip.style.left = '';
        return;
      }
      tip.classList.remove('docked');
      const tw = tip.offsetWidth;
      const th = tip.offsetHeight;
      const below = r.bottom + pad + 14 + th < window.innerHeight;
      const top = below ? r.bottom + pad + 14 : Math.max(12, r.top - pad - 14 - th);
      const left = Math.min(Math.max(12, r.left + r.width / 2 - tw / 2), window.innerWidth - tw - 12);
      tip.style.top = `${top}px`;
      tip.style.left = `${left}px`;
      tip.dataset.side = below ? 'below' : 'above';
    };
    tour.keys = (e) => {
      if (e.key === 'Escape') { e.preventDefault(); endTour(true); }
      else if (e.key === 'ArrowRight') { e.preventDefault(); next.click(); }
      else if (e.key === 'ArrowLeft') { e.preventDefault(); if (!back.disabled) back.click(); }
      else if (e.key === 'Tab') { // keep focus inside the tooltip
        const f = [...tip.querySelectorAll('button:not([disabled])')];
        const at = f.indexOf(document.activeElement);
        if (e.shiftKey && at <= 0) { e.preventDefault(); f[f.length - 1].focus(); }
        else if (!e.shiftKey && at === f.length - 1) { e.preventDefault(); f[0].focus(); }
      }
    };
    skip.addEventListener('click', () => endTour(true));
    back.addEventListener('click', () => { if (tour.i > 0) { tour.i -= 1; show(); } });
    next.addEventListener('click', () => {
      if (tour.i < tour.steps.length - 1) { tour.i += 1; show(); } else { endTour(true); }
    });
    spot.addEventListener('click', () => endTour(true));
    window.addEventListener('resize', tour.place);
    window.addEventListener('scroll', tour.place, true);
    document.addEventListener('keydown', tour.keys, true);
    show();
  }

  // ---------- Scroll-along guide ----------
  const GUIDE_HIDDEN = 'qp-guide-hidden';
  function startGuide() {
    const steps = [...document.querySelectorAll('ol.pipeline > li.step[data-guide]')];
    if (!steps.length) return;
    // Not an <aside>: the app styles every aside as its sidebar.
    const card = el('div', 'guide-card');
    card.setAttribute('role', 'complementary');
    card.setAttribute('aria-label', 'Guide to this purchase');
    const head = el('div', 'guide-head');
    const label = el('span', 'guide-label');
    const hide = el('button', 'guide-hide', 'Hide');
    hide.type = 'button';
    head.append(label, hide);
    const title = el('h2', 'guide-title');
    const what = el('p', 'guide-what');
    const lookWrap = el('p', 'guide-look');
    const lookLead = el('strong', null, 'Look for: ');
    const look = el('span');
    lookWrap.append(lookLead, look);
    const nextBtn = el('button', 'guide-next', 'Next step ↓');
    nextBtn.type = 'button';
    card.append(head, title, what, lookWrap, nextBtn);
    const pill = el('button', 'guide-pill', '? Guide');
    pill.type = 'button';
    pill.setAttribute('aria-label', 'Show the guide to this purchase');
    document.body.append(card, pill);

    let current = -1;
    const set = (i) => {
      if (i === current || i < 0) return;
      current = i;
      steps.forEach((s, k) => s.classList.toggle('guide-active', k === i));
      const s = steps[i];
      label.textContent = `Step ${i + 1} of ${steps.length}`;
      title.textContent = s.querySelector('.step-head h3')?.textContent || '';
      what.textContent = s.dataset.guide;
      look.textContent = s.dataset.guideLook || '';
      lookWrap.hidden = !s.dataset.guideLook;
      nextBtn.hidden = i === steps.length - 1;
    };
    const visible = (show) => {
      card.hidden = !show;
      pill.hidden = show;
      steps.forEach((s) => s.classList.toggle('guide-on', show));
      if (show) store.del('localStorage', GUIDE_HIDDEN);
      else store.set('localStorage', GUIDE_HIDDEN, '1');
    };
    hide.addEventListener('click', () => { visible(false); pill.focus(); });
    pill.addEventListener('click', () => { visible(true); hide.focus(); });
    nextBtn.addEventListener('click', () => {
      const n = steps[Math.min(current + 1, steps.length - 1)];
      n.scrollIntoView({ block: 'start', behavior: reduced() ? 'auto' : 'smooth' });
    });

    // Explain the step under a reading line 45% down the viewport (the last one whose top has
    // passed it), recomputed once per frame while scrolling.
    let queued = false;
    const pick = () => {
      queued = false;
      const line = window.innerHeight * 0.45;
      let hit = 0;
      steps.forEach((s, k) => { if (s.getBoundingClientRect().top <= line) hit = k; });
      set(hit);
    };
    const onScroll = () => { if (!queued) { queued = true; window.requestAnimationFrame(pick); } };
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll);
    pick();
    visible(store.get('localStorage', GUIDE_HIDDEN) !== '1');
  }

  // ---------- Entry points ----------
  function init() {
    if (document.body.dataset.auth !== 'true') return;
    const header = document.querySelector('main > header .runtime-badges');
    if (header && document.querySelector('#agent-request')) {
      const btn = el('button', 'tour-launch', '✦ Take the tour');
      btn.type = 'button';
      btn.addEventListener('click', () => startTour(0, btn));
      header.prepend(btn);
    }
    startGuide();
    const resume = store.get('sessionStorage', TOUR_STEP);
    if (resume !== null) startTour(Number(resume) || 0, null);
    else if (!store.get('localStorage', TOUR_DONE) && document.querySelector('#agent-request')) startTour(0, null);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();

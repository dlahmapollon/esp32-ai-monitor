(function () {
  'use strict';
  var models = { cyd: { label: 'CYD', resolution: '320 × 240 px', index: '01' }, s3: { label: 'ESP32-S3', resolution: '480 × 480 px', index: '02' } };
  var names = { de: { blue: 'Kobaltblau', graphite: 'Graphit', terracotta: 'Terrakotta' }, en: { blue: 'Cobalt blue', graphite: 'Graphite', terracotta: 'Terracotta' } };
  var model, colour, request = 0;
  var image = document.getElementById('product-image');
  var frame = image.closest('figure');
  var error = document.getElementById('product-error');
  var loading = document.getElementById('product-loading');
  var label = document.getElementById('product-name');
  var shown = { model: 's3', colour: 'graphite' };
  function readLocation() {
    var query = new URLSearchParams(window.location.search);
    model = Object.prototype.hasOwnProperty.call(models, query.get('model')) ? query.get('model') : 's3';
    colour = Object.prototype.hasOwnProperty.call(names.de, query.get('color')) ? query.get('color') : 'graphite';
  }
  function describe() {
    var language = document.documentElement.lang === 'en' ? 'en' : 'de';
    label.textContent = models[model].label + ' / ' + names[language][colour];
    image.alt = (language === 'de' ? 'KI-Produktvisualisierung: ' : 'AI product visualisation: ') + models[shown.model].label + ' / ' + names[language][shown.colour];
    document.getElementById('product-resolution').textContent = models[model].resolution;
  }
  function update() {
    var version = ++request;
    var desiredModel = model, desiredColour = colour;
    var src = 'assets/renders/' + (model === 's3' ? 's3-claude' : 'cyd') + '-' + colour + '.jpg';
    describe();
    document.querySelectorAll('[data-model]').forEach(function (button) {
      button.setAttribute('aria-pressed', String(button.dataset.model === model));
    });
    document.querySelectorAll('[data-color]').forEach(function (button) {
      button.setAttribute('aria-pressed', String(button.dataset.color === colour));
    });
    error.hidden = true;
    if (image.getAttribute('src') === src && image.complete && image.naturalWidth) {
      loading.hidden = true;
      frame.setAttribute('aria-busy', 'false');
      return;
    }
    frame.setAttribute('aria-busy', 'true');
    loading.hidden = false;
    var next = new Image();
    next.onload = function () {
      if (version !== request) return;
      image.src = src;
      shown = { model: desiredModel, colour: desiredColour };
      describe();
      loading.hidden = true;
      frame.setAttribute('aria-busy', 'false');
    };
    next.onerror = function () {
      if (version !== request) return;
      loading.hidden = true;
      frame.setAttribute('aria-busy', 'false');
      error.hidden = false;
    };
    next.src = src;
  }
  function saveLocation() {
    var url = new URL(window.location.href);
    if (model === 's3') url.searchParams.delete('model'); else url.searchParams.set('model', model);
    if (colour === 'graphite') url.searchParams.delete('color'); else url.searchParams.set('color', colour);
    if (url.href !== window.location.href) history.pushState(null, '', url);
  }
  document.querySelectorAll('[data-model]').forEach(function (button) {
    button.addEventListener('click', function () { model = button.dataset.model; saveLocation(); update(); });
  });
  document.querySelectorAll('[data-color]').forEach(function (button) {
    button.addEventListener('click', function () { colour = button.dataset.color; saveLocation(); update(); });
  });
  document.addEventListener('aimonitor:language', update);
  window.addEventListener('popstate', function () {
    readLocation();
    document.dispatchEvent(new Event('aimonitor:history'));
  });
  readLocation();
  update();
})();

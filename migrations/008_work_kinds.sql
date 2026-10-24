-- 008 · Trabajo más allá del código (P15.5.S5, ADR-0020).
-- Tres profiles nuevos para proyectos que no son de código. Mismos pesos de siempre
-- (fit, context, health, quota, scarcity, failures, load, speed); los puntajes por modelo los
-- siembra el router al arrancar, como con los demás.

INSERT OR IGNORE INTO profiles (id, description, weights_json, builtin) VALUES
    ('@write',    'Writing and editing documents',                     '{"fit":0.9,"context":0.4,"health":0.4,"quota":0.4,"scarcity":0.4,"failures":0.4,"load":0.2,"speed":0.1}', 1),
    ('@media',    'Orchestrating tools for video, audio and images',   '{"fit":1.0,"context":0.3,"health":0.5,"quota":0.4,"scarcity":0.4,"failures":0.5,"load":0.2,"speed":0.1}', 1),
    ('@research', 'Research and analysis',                             '{"fit":1.1,"context":0.5,"health":0.4,"quota":0.3,"scarcity":0.3,"failures":0.4,"load":0.1,"speed":0.0}', 1);

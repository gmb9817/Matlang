f = figure(66);
main_pie = pie([2, 1, 3]);
limits = axis();
current = gcf();
clf(f);
labeled_pie = pie([2, 1, 3], {'North', 'East', 'West'});
relief_pie = pie([2, 1, 3], [0, 1, 0]);
limits_after = axis();

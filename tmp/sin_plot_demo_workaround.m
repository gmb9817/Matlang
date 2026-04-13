x = linspace(0, 2*pi, 100);
y = sin(x);

figure();
h = plot(x, y);
set(h, 'LineWidth', 2);
title('Sinusoidal Wave');
xlabel('x');
ylabel('sin(x)');
grid on;

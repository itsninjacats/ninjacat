// Spike: uruchamianie skryptow uzytkownika w JS po stronie serwera.
//
// Pytanie, na ktore odpowiada ten program: jak dzialaja te "skrypty w JS"
// w narzedziach typu Datadog on-call czy Workflow Automation - jak wstrzykuje
// sie do nich dane i jak nie dac sie przy tym zabic.
//
// Dwie czesci:
//  1. Piaskownica - limit czasu, brak dostepu do swiata.
//  2. Kontekst - wstrzykniecie danych, funkcji gospodarza i odczyt wyniku.
//
// Uruchomienie:  go run .
package main

import (
	"encoding/json"
	"fmt"
	"time"

	"github.com/dop251/goja"
)

// ---------------------------------------------------------------------------
// 1. Piaskownica
// ---------------------------------------------------------------------------

// uruchomZLimitem odpala skrypt i przerywa go po zadanym czasie.
//
// Interrupt mozna wolac z innej goroutine - interpreter sprawdza flage miedzy
// krokami i przerywa wykonanie zwyklym bledem. Proces zyje dalej.
//
// UWAGA: Interrupt przerywa JavaScript, nie kod Go. Jesli udostepnisz funkcje
// gospodarza, ktora blokuje na 5 sekund, skrypt bedzie w niej siedzial mimo
// limitu. Funkcje, ktore wstawiasz, musza same pilnowac swojego czasu.
func uruchomZLimitem(skrypt string, limit time.Duration) (goja.Value, error) {
	vm := goja.New()

	t := time.AfterFunc(limit, func() {
		vm.Interrupt("przekroczono limit czasu")
	})
	defer t.Stop()

	return vm.RunString(skrypt)
}

func demoPiaskownica() {
	fmt.Println("=== 1. PIASKOWNICA ===")

	przypadki := []struct {
		opis   string
		skrypt string
	}{
		{"zwykly skrypt", "var s=0; for (var i=0;i<1000;i++) s+=i; s"},
		{"petla nieskonczona", "while(true){}"},
		{"zeranie pamieci", "var a=[]; while(true){ a.push('x'.repeat(1000)); }"},
		{"proba wyjscia w swiat", "fetch('http://example.com')"},
		{"proba czytania plikow", "require('fs').readFileSync('/etc/passwd')"},
	}

	for _, p := range przypadki {
		start := time.Now()
		v, err := uruchomZLimitem(p.skrypt, 200*time.Millisecond)
		wynik := fmt.Sprintf("%v", v)
		if err != nil {
			wynik = "BLAD: " + err.Error()
		}
		fmt.Printf("   %-24s %-60s (%v)\n", p.opis, wynik, time.Since(start).Round(time.Millisecond))
	}
	fmt.Println()
	fmt.Println("   Srodowisko jest PUSTE, dopoki czegos nie wstawisz - nie ma")
	fmt.Println("   fetch, require, process ani dostepu do plikow.")
	fmt.Println()
}

// ---------------------------------------------------------------------------
// 2. Kontekst: dane w jedna strone, decyzja w druga
// ---------------------------------------------------------------------------

// Alert to dane wstrzykiwane do skryptu.
type Alert struct {
	Monitor  string            `json:"monitor"`
	Status   string            `json:"status"`
	Severity int               `json:"severity"`
	Host     string            `json:"host"`
	Value    float64           `json:"value"`
	Tags     map[string]string `json:"tags"`
}

// Decyzja to struktura, ktora skrypt ma zwrocic.
type Decyzja struct {
	Kanal     string `json:"kanal"`
	Pilne     bool   `json:"pilne"`
	Wiadomosc string `json:"wiadomosc"`
}

// ewaluuj uruchamia skrypt routingu na pojedynczym alercie.
func ewaluuj(skrypt string, alert Alert, limit time.Duration) (*Decyzja, []string, error) {
	vm := goja.New()

	// Bez tego pola struktur widac w JS jako Host, Severity (po gowemu).
	// Z tym - uzywa tagow json, wiec w skrypcie wyglada naturalnie.
	vm.SetFieldNameMapper(goja.TagFieldNameMapper("json", true))

	var logi []string

	// Dane wejsciowe - skrypt zobaczy globalne `alert`.
	vm.Set("alert", alert)

	// Funkcje gospodarza. To jest CALA kontrola nad tym, co skrypt moze
	// zrobic: nie ma fetch, bo go nie dodalismy.
	//
	// console.log nie pisze na ekran - zbiera do slice'a, zeby uzytkownik
	// dostal swoje logi w UI, a my decydowali co z nimi.
	vm.Set("console", map[string]any{
		"log": func(s string) { logi = append(logi, s) },
	})
	vm.Set("godzina", func() int { return time.Now().Hour() })

	t := time.AfterFunc(limit, func() { vm.Interrupt("przekroczono limit czasu") })
	defer t.Stop()

	// Opakowanie w funkcje, zeby skrypt uzytkownika mogl uzywac `return`.
	v, err := vm.RunString("(function(){\n" + skrypt + "\n})()")
	if err != nil {
		return nil, logi, err
	}

	// Wynik wraca jako map[string]interface{}; przez JSON do naszej struktury.
	raw, err := json.Marshal(v.Export())
	if err != nil {
		return nil, logi, fmt.Errorf("nie moge odczytac wyniku: %w", err)
	}
	var d Decyzja
	if err := json.Unmarshal(raw, &d); err != nil {
		return nil, logi, fmt.Errorf("skrypt zwrocil cos innego niz Decyzja: %w", err)
	}
	return &d, logi, nil
}

func demoKontekst() {
	fmt.Println("=== 2. KONTEKST ===")

	alert := Alert{
		Monitor: "CPU wysokie", Status: "CRITICAL", Severity: 4,
		Host: "node-7", Value: 97.3,
		Tags: map[string]string{"env": "prod", "team": "platform"},
	}

	// Tak moglby wygladac skrypt napisany przez uzytkownika w UI.
	skrypt := `
		console.log("dostalem alert z " + alert.host + ", wartosc " + alert.value);

		if (alert.tags.env !== "prod") {
			return { kanal: "#szum", pilne: false, wiadomosc: "nieprodukcyjne" };
		}

		var wNocy = godzina() < 6 || godzina() >= 22;
		return {
			kanal:     wNocy && alert.severity >= 4 ? "pager-" + alert.tags.team : "#alerty",
			pilne:     wNocy && alert.severity >= 4,
			wiadomosc: alert.monitor + " na " + alert.host + " (" + alert.value + "%)"
		};
	`

	d, logi, err := ewaluuj(skrypt, alert, 200*time.Millisecond)

	fmt.Println("   co skrypt wypisal przez console.log:")
	for _, l := range logi {
		fmt.Println("      " + l)
	}
	if err != nil {
		fmt.Printf("   BLAD: %v\n", err)
		return
	}
	fmt.Println("   co zwrocil do Go:")
	fmt.Printf("      kanal:     %s\n", d.Kanal)
	fmt.Printf("      pilne:     %v\n", d.Pilne)
	fmt.Printf("      wiadomosc: %s\n", d.Wiadomosc)
}

func main() {
	demoPiaskownica()
	demoKontekst()
}

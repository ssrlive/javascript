{
    const today = new Date();
    const endYear = new Date(2000, 11, 31, 23, 59, 59, 999); // Set date and month
    endYear.setFullYear(today.getFullYear()); // Set year to this year
    const msPerDay = 24 * 60 * 60 * 1000; // Number of milliseconds in a day
    let daysLeft = (endYear.getTime() - today.getTime()) / msPerDay;
    daysLeft = Math.round(daysLeft); // Return the number of days left in this year
    console.log(`There are ${daysLeft} days left until the end of this year`);

    const ipoDate = new Date();
    ipoDate.setTime(Date.parse("Aug 9, 1995"));
    console.log(`The IPO of Amazon was on ${ipoDate.toDateString()}`);

    function JSClock() {
        const time = new Date();
        const hour = time.getHours();
        const minute = time.getMinutes();
        const second = time.getSeconds();
        let temp = String(hour % 12);
        if (temp === "0") {
            temp = "12";
        }
        temp += (minute < 10 ? ":0" : ":") + minute;
        temp += (second < 10 ? ":0" : ":") + second;
        temp += hour >= 12 ? " P.M." : " A.M.";
        return temp;
    }

    console.log(`The current time is: ${JSClock()}`);
}

{
    console.log("=== Testing Date modification via method... ===");

    function incrementDay(date) {
        return date.setDate(date.getDate() + 1);
    }
    const date = new Date();
    const newDay = incrementDay(date);
    console.log(newDay);
    // The old date is modified as well!?
    console.log(date);
}
